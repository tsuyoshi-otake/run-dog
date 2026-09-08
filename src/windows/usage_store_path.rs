//! Pinned, no-reparse filesystem boundary for the usage store.
//!
//! Pin ancestors against reparse insertion and replacement. The final directory
//! allows write sharing (required for atomic rename), but all its child operations
//! are handle-relative: changes to its path or reparse data cannot redirect them.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_NO_MORE_FILES, GENERIC_READ, GENERIC_WRITE, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
    },
    Globalization::{CompareStringOrdinal, CSTR_EQUAL},
    Storage::FileSystem::{
        FileDispositionInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
        SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    },
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FileRenameInformation, NtCreateFile, NtSetInformationFile, FILE_CREATE,
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF,
            FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT,
        },
    },
    Win32::System::IO::IO_STATUS_BLOCK,
};

pub(super) struct PinnedDirectory {
    path: PathBuf,
    handles: Vec<File>,
}

impl PinnedDirectory {
    pub(super) fn open(
        root: &Path,
        create: bool,
        denied: &[PathBuf],
        delete: bool,
    ) -> Option<Self> {
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            || forbidden(root, denied)
        {
            return None;
        }
        let mut handles: Vec<File> = Vec::new();
        for path in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
            if forbidden(path, denied) {
                return None;
            }
            // Metadata-only handles do not participate in Windows share-access
            // checks. FILE_LIST_DIRECTORY is necessary to pin the directory.
            let access = FILE_LIST_DIRECTORY
                | FILE_READ_ATTRIBUTES
                | if delete && path == root { DELETE } else { 0 };
            let sharing = FILE_SHARE_READ | if path == root { FILE_SHARE_WRITE } else { 0 };
            let file = if let Some(parent) = handles.last() {
                // Resolve and create each child relative to the pinned parent,
                // including when a DOS drive alias changes during this walk.
                open_relative(parent, path.file_name()?, access, sharing, create, true)?
            } else {
                OpenOptions::new()
                    .access_mode(access)
                    .share_mode(sharing)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(path)
                    .ok()?
            };
            let info = file_info(&file)?;
            if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                != FILE_ATTRIBUTE_DIRECTORY
                || forbidden(&final_path(&file)?, denied)
            {
                return None;
            }
            handles.push(file);
        }
        Some(Self {
            path: root.to_path_buf(),
            handles,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn read(&self, path: &Path) -> Option<Vec<u8>> {
        if path.parent() != Some(self.path()) {
            return None;
        }
        let mut file = self.open_regular(path, GENERIC_READ)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    pub(super) fn write_atomically(&self, tmp: &Path, destination: &Path, bytes: &[u8]) -> bool {
        if tmp.parent() != Some(self.path()) || destination.parent() != Some(self.path()) {
            return false;
        }
        if let Some(file) = self.open_regular(tmp, FILE_READ_ATTRIBUTES | DELETE) {
            if !delete_handle(&file) {
                return false;
            }
        }
        // CREATE_NEW cannot truncate a planted symlink or hardlink. Retain the
        // exclusive handle through publication; never reopen the temporary name.
        let Some(mut file) = self.open_child(tmp, GENERIC_WRITE | DELETE, true) else {
            return false;
        };
        let destination_safe = self
            .open_child(destination, FILE_READ_ATTRIBUTES, false)
            .is_none_or(|file| regular(&file));
        if file.write_all(bytes).is_ok()
            && file.sync_all().is_ok()
            && destination_safe
            && self
                .handles
                .last()
                .is_some_and(|parent| rename_handle(&file, destination, parent, true))
        {
            return true;
        }
        let _ = delete_handle(&file);
        false
    }

    pub(super) fn remove_file(&self, path: &Path) -> bool {
        if path.parent() != Some(self.path()) {
            return false;
        }
        self.open_regular(path, FILE_READ_ATTRIBUTES | DELETE)
            .is_some_and(|file| delete_handle(&file))
    }

    fn open_regular(&self, path: &Path, access: u32) -> Option<File> {
        let file = self.open_child(path, access, false)?;
        regular(&file).then_some(file)
    }

    fn open_child(&self, path: &Path, access: u32, create: bool) -> Option<File> {
        if path.parent() != Some(self.path()) {
            return None;
        }
        open_relative(
            self.handles.last()?,
            path.file_name()?,
            access,
            if create { 0 } else { FILE_SHARE_READ },
            create,
            false,
        )
    }

    pub(super) fn entries(&self) -> Option<Vec<PathBuf>> {
        let directory = self.handles.last()?;
        let mut storage = vec![0_u64; 8192];
        let mut entries = Vec::new();
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    directory.as_raw_handle(),
                    class,
                    storage.as_mut_ptr().cast(),
                    (storage.len() * size_of::<u64>()) as u32,
                )
            };
            if ok == 0 {
                return (io::Error::last_os_error().raw_os_error()
                    == Some(ERROR_NO_MORE_FILES as i32))
                .then_some(entries);
            }
            class = FileIdBothDirectoryInfo;
            let mut offset = 0;
            loop {
                let capacity = storage.len() * size_of::<u64>();
                if offset + size_of::<FILE_ID_BOTH_DIR_INFO>() > capacity {
                    return None;
                }
                let info = unsafe {
                    &*storage
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>()
                };
                let bytes = info.FileNameLength as usize;
                if !bytes.is_multiple_of(2)
                    || offset + offset_of!(FILE_ID_BOTH_DIR_INFO, FileName) + bytes > capacity
                {
                    return None;
                }
                let name = unsafe {
                    std::slice::from_raw_parts(
                        ptr::addr_of!(info.FileName).cast::<u16>(),
                        bytes / 2,
                    )
                };
                let name = std::ffi::OsString::from_wide(name);
                if name != "." && name != ".." {
                    entries.push(self.path.join(name));
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                let next = info.NextEntryOffset as usize;
                if next < size_of::<FILE_ID_BOTH_DIR_INFO>() || !next.is_multiple_of(8) {
                    return None;
                }
                offset += next;
            }
        }
    }

    pub(super) fn rename_to(&self, destination: &Path, parent: &Self) -> bool {
        if destination.parent() != Some(parent.path()) {
            return false;
        }
        // The caller holds the destination parent guard for the whole rename.
        self.handles.last().is_some_and(|file| {
            parent
                .handles
                .last()
                .is_some_and(|parent| rename_handle(file, destination, parent, false))
        })
    }

    pub(super) fn remove_empty(&self) -> bool {
        self.handles.last().is_some_and(delete_handle)
    }
}

fn open_relative(
    parent: &File,
    name: &std::ffi::OsStr,
    access: u32,
    sharing: u32,
    create: bool,
    directory: bool,
) -> Option<File> {
    let mut name: Vec<_> = name.encode_wide().collect();
    if name.is_empty()
        || name == [b'.' as u16]
        || name == [b'.' as u16; 2]
        || name
            .iter()
            .any(|ch| [0, b':' as u16, b'\\' as u16, b'/' as u16].contains(ch))
    {
        return None;
    }
    let length = u16::try_from(name.len() * size_of::<u16>()).ok()?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        ..Default::default()
    };
    let mut handle = ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let disposition = if !create {
        FILE_OPEN
    } else if directory {
        FILE_OPEN_IF
    } else {
        FILE_CREATE
    };
    let kind = if directory {
        FILE_DIRECTORY_FILE
    } else {
        FILE_NON_DIRECTORY_FILE
    };
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attributes,
            &mut status,
            ptr::null(),
            0,
            sharing,
            disposition,
            FILE_OPEN_REPARSE_POINT | kind | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    (result >= 0).then(|| unsafe { File::from_raw_handle(handle) })
}

fn regular(file: &File) -> bool {
    file_info(file).is_some_and(|info| {
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
            && info.nNumberOfLinks == 1
    })
}

fn file_info(file: &File) -> Option<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0).then_some(info)
}

fn final_path(file: &File) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let needed = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), ptr::null_mut(), 0, 0) };
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0; needed as usize];
    let length =
        unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), needed, 0) };
    if length == 0 || length >= needed {
        return None;
    }
    Some(std::ffi::OsString::from_wide(&buf[..length as usize]).into())
}

fn delete_handle(file: &File) -> bool {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        ) != 0
    }
}

fn rename_handle(file: &File, destination: &Path, parent: &File, replace: bool) -> bool {
    let Some(name) = destination.file_name() else {
        return false;
    };
    let name: Vec<_> = name.encode_wide().collect();
    let bytes = offset_of!(FILE_RENAME_INFORMATION, FileName) + (name.len() + 1) * size_of::<u16>();
    // usize storage supplies the native pointer alignment of FILE_RENAME_INFORMATION.
    let mut storage = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut status = IO_STATUS_BLOCK::default();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (name.len() * size_of::<u16>()) as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        let result = NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        );
        result >= 0
    }
}

/// Resolve an existing prefix without creating a missing suffix. This handles
/// configured provider aliases and DOS short names before any directory creation.
fn resolved_candidate(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut suffix = Vec::new();
    for ancestor in absolute.ancestors() {
        if let Ok(mut resolved) = fs::canonicalize(ancestor) {
            for name in suffix.into_iter().rev() {
                resolved.push(name);
            }
            return Some(resolved);
        }
        suffix.push(ancestor.file_name()?.to_os_string());
    }
    None
}

fn comparable(path: &Path) -> Option<Vec<u16>> {
    let absolute = std::path::absolute(path).ok()?;
    let text = absolute.to_str()?.replace('/', "\\");
    let text = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    Some(text.trim_end_matches('\\').encode_utf16().collect())
}

fn under(candidate: &Path, denied: &Path) -> bool {
    let (Some(candidate), Some(denied)) = (comparable(candidate), comparable(denied)) else {
        return true;
    };
    candidate.len() >= denied.len()
        && (candidate.len() == denied.len() || candidate[denied.len()] == u16::from(b'\\'))
        && unsafe {
            CompareStringOrdinal(
                candidate.as_ptr(),
                denied.len() as i32,
                denied.as_ptr(),
                denied.len() as i32,
                1,
            )
        } == CSTR_EQUAL
}

pub(super) fn forbidden(root: &Path, denied: &[PathBuf]) -> bool {
    let resolved_root = resolved_candidate(root);
    denied.iter().any(|denied| {
        under(root, denied)
            || match (&resolved_root, resolved_candidate(denied)) {
                (Some(root), Some(denied)) => under(root, &denied),
                _ => false,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Storage::FileSystem::{
        DefineDosDeviceW, DDD_EXACT_MATCH_ON_REMOVE, DDD_NO_BROADCAST_SYSTEM, DDD_RAW_TARGET_PATH,
        DDD_REMOVE_DEFINITION,
    };

    struct DosAlias {
        name: Vec<u16>,
        targets: Vec<Vec<u16>>,
    }

    impl DosAlias {
        fn push(&mut self, target: &Path) {
            let target: Vec<_> = format!(r"\??\{}", target.display())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            assert_ne!(
                unsafe {
                    DefineDosDeviceW(
                        DDD_RAW_TARGET_PATH | DDD_NO_BROADCAST_SYSTEM,
                        self.name.as_ptr(),
                        target.as_ptr(),
                    )
                },
                0,
                "{}",
                io::Error::last_os_error()
            );
            self.targets.push(target);
        }
    }

    impl Drop for DosAlias {
        fn drop(&mut self) {
            for target in self.targets.iter().rev() {
                unsafe {
                    DefineDosDeviceW(
                        DDD_RAW_TARGET_PATH
                            | DDD_NO_BROADCAST_SYSTEM
                            | DDD_REMOVE_DEFINITION
                            | DDD_EXACT_MATCH_ON_REMOVE,
                        self.name.as_ptr(),
                        target.as_ptr(),
                    );
                }
            }
        }
    }

    #[test]
    fn component_directory_creation_stays_with_parent_after_dos_alias_retarget() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let device = format!("RunDogStoreTest{}-{nonce}", std::process::id());
        let base = std::env::temp_dir().join(&device);
        let safe = base.join("safe");
        let provider = base.join("provider");
        fs::create_dir_all(&safe).unwrap();
        fs::create_dir_all(&provider).unwrap();
        let mut alias = DosAlias {
            name: device.encode_utf16().chain(Some(0)).collect(),
            targets: Vec::new(),
        };
        alias.push(&safe);
        let alias_path = PathBuf::from(format!(r"\\.\{device}\"));
        let parent = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&alias_path)
            .unwrap();
        alias.push(&provider);
        // The former absolute-path operation really follows the new alias.
        fs::create_dir(alias_path.join("old-path-proof")).unwrap();
        assert!(provider.join("old-path-proof").is_dir());
        fs::remove_dir(provider.join("old-path-proof")).unwrap();
        let child = open_relative(
            &parent,
            std::ffi::OsStr::new("guarded"),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ,
            true,
            true,
        )
        .unwrap();
        assert!(safe.join("guarded").is_dir());
        assert!(!provider.join("guarded").exists());
        drop(child);
        drop(parent);
        drop(alias);
        fs::remove_dir_all(base).unwrap();
    }
}
