use super::{ResolvedTheme, TrayGlyph};

pub const TRAY_ICON_SIZE: usize = 32;
const BYTES_PER_PIXEL: usize = 4;
const FONT_WIDTH: usize = 5;
const FONT_HEIGHT: usize = 7;
const GAP: usize = 1;

/// Bottom-up 32×32 BGRA pixels matching the embedded dog frames.
#[must_use]
pub fn rasterize_tray_glyph(glyph: &TrayGlyph, theme: ResolvedTheme) -> Vec<u8> {
    let mut top_down = vec![0_u8; TRAY_ICON_SIZE * TRAY_ICON_SIZE * BYTES_PER_PIXEL];
    let color = glyph_color(theme);
    blit_text(&mut top_down, glyph.tag, 3, 1, color);
    let value_scale = if glyph.value.chars().count() <= 2 {
        2
    } else {
        1
    };
    blit_text(&mut top_down, &glyph.value, 14, value_scale, color);
    flip_to_bottom_up(&top_down)
}

fn glyph_color(theme: ResolvedTheme) -> [u8; 4] {
    if theme.is_light() {
        [40, 32, 28, 255]
    } else {
        [250, 247, 245, 255]
    }
}

fn blit_text(pixels: &mut [u8], text: &str, top: usize, scale: usize, color: [u8; 4]) {
    let chars: Vec<char> = text
        .chars()
        .filter(|ch| glyph_rows(*ch).is_some())
        .collect();
    if chars.is_empty() || scale == 0 {
        return;
    }
    let width = text_width(chars.len(), scale);
    if width > TRAY_ICON_SIZE {
        return;
    }
    let left = (TRAY_ICON_SIZE - width) / 2;
    for (index, ch) in chars.into_iter().enumerate() {
        let Some(rows) = glyph_rows(ch) else {
            continue;
        };
        let origin_x = left + index * (FONT_WIDTH * scale + GAP * scale);
        blit_glyph(pixels, origin_x, top, scale, rows, color);
    }
}

fn text_width(len: usize, scale: usize) -> usize {
    len * FONT_WIDTH * scale + len.saturating_sub(1) * GAP * scale
}

fn blit_glyph(
    pixels: &mut [u8],
    origin_x: usize,
    origin_y: usize,
    scale: usize,
    rows: [u8; FONT_HEIGHT],
    color: [u8; 4],
) {
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..FONT_WIDTH {
            if bits & (1 << (FONT_WIDTH - 1 - column)) == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    plot(
                        pixels,
                        origin_x + column * scale + dx,
                        origin_y + row * scale + dy,
                        color,
                    );
                }
            }
        }
    }
}

fn plot(pixels: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    if x >= TRAY_ICON_SIZE || y >= TRAY_ICON_SIZE {
        return;
    }
    let index = (y * TRAY_ICON_SIZE + x) * BYTES_PER_PIXEL;
    pixels[index..index + BYTES_PER_PIXEL].copy_from_slice(&color);
}

fn flip_to_bottom_up(top_down: &[u8]) -> Vec<u8> {
    let stride = TRAY_ICON_SIZE * BYTES_PER_PIXEL;
    let mut bottom_up = vec![0_u8; top_down.len()];
    for y in 0..TRAY_ICON_SIZE {
        let src = y * stride;
        let dst = (TRAY_ICON_SIZE - 1 - y) * stride;
        bottom_up[dst..dst + stride].copy_from_slice(&top_down[src..src + stride]);
    }
    bottom_up
}

/// 5×7 glyphs, high bit is the left column.
fn glyph_rows(ch: char) -> Option<[u8; FONT_HEIGHT]> {
    Some(match ch.to_ascii_uppercase() {
        '0' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b01110, 0b00000, 0b00000, 0b00000,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{rasterize_tray_glyph, TRAY_ICON_SIZE};
    use crate::core::{ResolvedTheme, TrayGlyph};
    use proptest::prelude::*;

    fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
        let row = TRAY_ICON_SIZE - 1 - y;
        let index = (row * TRAY_ICON_SIZE + x) * 4;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    }

    fn opaque_count(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .count()
    }

    #[test]
    fn c2_raster_is_32x32_with_transparent_corners_and_visible_digits() {
        let glyph = TrayGlyph {
            tag: "CPU",
            value: "42".to_owned(),
        };
        let dark = rasterize_tray_glyph(&glyph, ResolvedTheme::Dark);
        let light = rasterize_tray_glyph(&glyph, ResolvedTheme::Light);
        assert_eq!(dark.len(), TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4);
        assert_eq!(pixel(&dark, 0, 0)[3], 0);
        assert_eq!(pixel(&dark, 31, 0)[3], 0);
        assert_eq!(pixel(&dark, 0, 31)[3], 0);
        assert!(opaque_count(&dark) > 20);
        assert_eq!(opaque_count(&dark), opaque_count(&light));
        let painted = (0..TRAY_ICON_SIZE)
            .flat_map(|y| (0..TRAY_ICON_SIZE).map(move |x| (x, y)))
            .find(|&(x, y)| pixel(&dark, x, y)[3] != 0)
            .expect("digits paint at least one pixel");
        assert_ne!(
            pixel(&dark, painted.0, painted.1),
            pixel(&light, painted.0, painted.1)
        );
    }

    #[test]
    fn c2_three_digit_values_stay_inside_the_icon() {
        let glyph = TrayGlyph {
            tag: "MEM",
            value: "100".to_owned(),
        };
        let pixels = rasterize_tray_glyph(&glyph, ResolvedTheme::Dark);
        assert_eq!(pixels.len(), TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4);
        assert!(opaque_count(&pixels) > 20);
        assert_eq!(pixel(&pixels, 0, 31)[3], 0);
    }

    #[test]
    fn component_limit_tags_paint_new_letters() {
        for tag in ["C5H", "C7D", "FAB", "X5H", "X7D"] {
            let glyph = TrayGlyph {
                tag,
                value: "73".to_owned(),
            };
            let pixels = rasterize_tray_glyph(&glyph, ResolvedTheme::Dark);
            assert!(
                opaque_count(&pixels) > 20,
                "{tag} should paint visible pixels"
            );
        }
    }

    #[test]
    fn component_unknown_characters_do_not_paint() {
        let glyph = TrayGlyph {
            tag: "??",
            value: String::new(),
        };
        let pixels = rasterize_tray_glyph(&glyph, ResolvedTheme::Dark);
        assert_eq!(opaque_count(&pixels), 0);
    }

    proptest! {
        #[test]
        fn pbt_percent_glyphs_have_fixed_size(
            value in 0u8..=100,
            dark in any::<bool>(),
        ) {
            let glyph = TrayGlyph {
                tag: "GPU",
                value: value.to_string(),
            };
            let theme = if dark {
                ResolvedTheme::Dark
            } else {
                ResolvedTheme::Light
            };
            let pixels = rasterize_tray_glyph(&glyph, theme);
            prop_assert_eq!(pixels.len(), TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4);
            prop_assert!(opaque_count(&pixels) > 0);
        }
    }
}
