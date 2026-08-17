fn main() {
    println!("cargo:rerun-if-env-changed=RUN_DOG_UPDATE_REPOSITORY");
    println!("cargo:rerun-if-changed=assets/rundog.ico");
    println!("cargo:rerun-if-changed=assets/rundog.rc");

    if let Ok(repository) = std::env::var("RUN_DOG_UPDATE_REPOSITORY") {
        println!("cargo:rustc-env=RUN_DOG_UPDATE_REPOSITORY={repository}");
    }

    write_version_resource();

    embed_resource::compile("assets/rundog.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("embed RunDog application icon");

    let version_rc =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("version.rc");
    embed_resource::compile(&version_rc, embed_resource::NONE)
        .manifest_optional()
        .expect("embed RunDog VERSIONINFO");
}

fn write_version_resource() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    let patch = parts.next().unwrap_or("0");
    let file_ver = format!("{major},{minor},{patch},0");
    let rc = format!(
        r#"1 VERSIONINFO
FILEVERSION {file_ver}
PRODUCTVERSION {file_ver}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Tsuyoshi Otake"
            VALUE "FileDescription", "RunDog"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "RunDog"
            VALUE "LegalCopyright", "Copyright (c) 2026 Tsuyoshi Otake"
            VALUE "OriginalFilename", "RunDog.exe"
            VALUE "ProductName", "RunDog"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let path =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("version.rc");
    std::fs::write(&path, rc).expect("write VERSIONINFO resource");
}
