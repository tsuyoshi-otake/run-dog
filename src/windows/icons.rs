use std::{ffi::c_void, mem::size_of, ptr};

use windows_sys::Win32::{
    Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    },
    UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
};

use crate::core::ResolvedTheme;

const FRAME_WIDTH: i32 = 32;
const FRAME_HEIGHT: i32 = 32;
const BYTES_PER_PIXEL: usize = 4;

/// HICONs created once during startup and reused for every tray update.
pub struct IconFrames {
    dark: [OwnedIcon; 3],
    light: [OwnedIcon; 3],
}

impl IconFrames {
    pub fn load() -> Result<Self, String> {
        let dark_pixels = [
            parse_embedded_bmp(include_bytes!("../../dark-dog-1.ico"))?,
            parse_embedded_bmp(include_bytes!("../../dark-dog-2.ico"))?,
            parse_embedded_bmp(include_bytes!("../../dark-dog-3.ico"))?,
        ];
        let light_pixels = dark_pixels.clone().map(invert_visible_pixels);

        Ok(Self {
            dark: dark_pixels
                .map(|pixels| create_icon(&pixels))
                .transpose_array()?,
            light: light_pixels
                .map(|pixels| create_icon(&pixels))
                .transpose_array()?,
        })
    }

    #[must_use]
    pub fn icon(&self, theme: ResolvedTheme, frame: usize) -> HICON {
        let index = frame % self.dark.len();
        match theme {
            ResolvedTheme::Light => self.light[index].raw(),
            ResolvedTheme::Dark => self.dark[index].raw(),
        }
    }
}

/// Small local helper because stable Rust does not yet provide array
/// `transpose` for arbitrary fixed lengths.
trait TransposeArray<T, E> {
    fn transpose_array(self) -> Result<[T; 3], E>;
}

impl<T, E> TransposeArray<T, E> for [Result<T, E>; 3] {
    fn transpose_array(self) -> Result<[T; 3], E> {
        let [first, second, third] = self;
        Ok([first?, second?, third?])
    }
}

struct OwnedIcon(HICON);

impl OwnedIcon {
    #[must_use]
    const fn raw(&self) -> HICON {
        self.0
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }
}

#[derive(Clone, Debug)]
struct BgraBitmap {
    pixels: Vec<u8>,
}

fn parse_embedded_bmp(bytes: &[u8]) -> Result<BgraBitmap, String> {
    const FILE_HEADER_LENGTH: usize = 14;
    const DIB_HEADER_LENGTH: usize = 40;
    const DIB_HEADER_SIZE_OFFSET: usize = 14;
    const BITS_OFFSET_OFFSET: usize = 10;
    const WIDTH_OFFSET: usize = 18;
    const HEIGHT_OFFSET: usize = 22;
    const BITS_PER_PIXEL_OFFSET: usize = 28;
    const COMPRESSION_OFFSET: usize = 30;
    const PIXEL_BYTES: usize = FRAME_WIDTH as usize * FRAME_HEIGHT as usize * BYTES_PER_PIXEL;

    if bytes.len() < FILE_HEADER_LENGTH + DIB_HEADER_LENGTH || &bytes[..2] != b"BM" {
        return Err("embedded dog frame is not a BMP file".to_owned());
    }

    let dib_header_size = read_u32(bytes, DIB_HEADER_SIZE_OFFSET)? as usize;
    let bits_offset = read_u32(bytes, BITS_OFFSET_OFFSET)? as usize;
    let width = read_i32(bytes, WIDTH_OFFSET)?;
    let height = read_i32(bytes, HEIGHT_OFFSET)?;
    let bits_per_pixel = read_u16(bytes, BITS_PER_PIXEL_OFFSET)?;
    let compression = read_u32(bytes, COMPRESSION_OFFSET)?;
    let end = bits_offset
        .checked_add(PIXEL_BYTES)
        .ok_or_else(|| "embedded dog frame pixel range overflows".to_owned())?;

    if dib_header_size < DIB_HEADER_LENGTH
        || width != FRAME_WIDTH
        || height != FRAME_HEIGHT
        || bits_per_pixel != 32
        || (compression != 0 && compression != 3)
        || end > bytes.len()
    {
        return Err("embedded dog frame must be a 32x32 32-bit BMP".to_owned());
    }

    Ok(BgraBitmap {
        pixels: bytes[bits_offset..end].to_vec(),
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let slice = bytes
        .get(offset..offset + size_of::<u16>())
        .ok_or_else(|| "embedded dog frame header is truncated".to_owned())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset + size_of::<u32>())
        .ok_or_else(|| "embedded dog frame header is truncated".to_owned())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, String> {
    Ok(read_u32(bytes, offset)? as i32)
}

fn invert_visible_pixels(mut bitmap: BgraBitmap) -> BgraBitmap {
    for pixel in bitmap.pixels.chunks_exact_mut(BYTES_PER_PIXEL) {
        if pixel[3] != 0 {
            pixel[0] = 255 - pixel[0];
            pixel[1] = 255 - pixel[1];
            pixel[2] = 255 - pixel[2];
        }
    }
    bitmap
}

fn create_icon(bitmap: &BgraBitmap) -> Result<OwnedIcon, String> {
    let pixel_count = bitmap.pixels.len();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: FRAME_WIDTH,
            biHeight: FRAME_HEIGHT,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: pixel_count as u32,
            ..BITMAPINFOHEADER::default()
        },
        ..BITMAPINFO::default()
    };
    let mut destination: *mut c_void = ptr::null_mut();
    let color_bitmap = unsafe {
        CreateDIBSection(
            ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut destination,
            ptr::null_mut(),
            0,
        )
    };
    if color_bitmap.is_null() || destination.is_null() {
        return Err("could not create a colour bitmap for a dog frame".to_owned());
    }

    unsafe {
        ptr::copy_nonoverlapping(
            bitmap.pixels.as_ptr(),
            destination.cast::<u8>(),
            pixel_count,
        );
    }

    let mask_bits = [0_u8; (FRAME_WIDTH as usize / 8) * FRAME_HEIGHT as usize];
    let mask_bitmap = unsafe {
        CreateBitmap(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            1,
            1,
            mask_bits.as_ptr().cast::<c_void>(),
        )
    };
    if mask_bitmap.is_null() {
        let _ = unsafe { DeleteObject(color_bitmap) };
        return Err("could not create a transparency mask for a dog frame".to_owned());
    }

    let icon_info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bitmap,
        hbmColor: color_bitmap,
    };
    let icon = unsafe { CreateIconIndirect(&icon_info) };
    let _ = unsafe { DeleteObject(mask_bitmap) };
    let _ = unsafe { DeleteObject(color_bitmap) };
    if icon.is_null() {
        return Err("could not create a dog icon".to_owned());
    }
    Ok(OwnedIcon(icon))
}

#[cfg(test)]
mod tests {
    use super::{invert_visible_pixels, parse_embedded_bmp, FRAME_HEIGHT, FRAME_WIDTH};

    #[test]
    fn component_embedded_dog_frame_has_expected_bitmap_contract() {
        let bitmap = parse_embedded_bmp(include_bytes!("../../dark-dog-1.ico"))
            .expect("placed dog design must be a supported BMP");
        assert_eq!(
            bitmap.pixels.len(),
            FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 4
        );
    }

    #[test]
    fn component_parser_rejects_short_and_wrong_magic_inputs() {
        assert!(parse_embedded_bmp(&[]).is_err());
        assert!(parse_embedded_bmp(&[0; 64]).is_err());
    }

    #[test]
    fn component_parser_rejects_invalid_dimensions_and_pixel_format() {
        let source = include_bytes!("../../dark-dog-1.ico");
        let mut invalid_width = source.to_vec();
        invalid_width[18..22].copy_from_slice(&31_i32.to_le_bytes());
        assert!(parse_embedded_bmp(&invalid_width).is_err());

        let mut invalid_compression = source.to_vec();
        invalid_compression[30..34].copy_from_slice(&2_u32.to_le_bytes());
        assert!(parse_embedded_bmp(&invalid_compression).is_err());
    }

    #[test]
    fn component_light_variant_preserves_alpha_and_changes_visible_colour() {
        let original =
            parse_embedded_bmp(include_bytes!("../../dark-dog-1.ico")).expect("valid input");
        let light = invert_visible_pixels(original.clone());
        for (before, after) in original
            .pixels
            .chunks_exact(4)
            .zip(light.pixels.chunks_exact(4))
        {
            assert_eq!(before[3], after[3]);
            if before[3] != 0 {
                assert_eq!(u16::from(before[0]) + u16::from(after[0]), 255);
            }
        }
    }
}
