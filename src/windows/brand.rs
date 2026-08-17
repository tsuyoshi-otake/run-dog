//! Official Claude and OpenAI marks, same 24×24 paths as otak-usage `brandIcons.ts`.
//!
//! Paths come from simple-icons (CC0). The marks remain property of their owners.

use windows_sys::Win32::{
    Foundation::{COLORREF, POINT, RECT},
    Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, GetStockObject, PolyPolygon, SelectObject, SetPolyFillMode,
        ALTERNATE, NULL_PEN, WINDING,
    },
};

const VIEW: f32 = 24.0;
const CURVE_STEPS: u32 = 8;

pub fn draw_claude(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
    fill_path(hdc, rect, color, CLAUDE_SVG_PATH, WINDING);
}

pub fn draw_openai(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
    fill_path(hdc, rect, color, OPENAI_SVG_PATH, ALTERNATE);
}

fn fill_path(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    path: &str,
    mode: i32,
) {
    let contours = flatten_svg_path(path);
    let mut points = Vec::new();
    let mut counts = Vec::new();
    for contour in contours {
        let mapped = map_contour(&contour, rect);
        if mapped.len() < 3 {
            continue;
        }
        counts.push(mapped.len() as i32);
        points.extend(mapped);
    }
    if counts.is_empty() {
        return;
    }
    let brush = unsafe { CreateSolidBrush(color) };
    if brush.is_null() {
        return;
    }
    let previous_brush = unsafe { SelectObject(hdc, brush) };
    let previous_pen = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
    let previous_mode = unsafe { SetPolyFillMode(hdc, mode) };
    let _ = unsafe { PolyPolygon(hdc, points.as_ptr(), counts.as_ptr(), counts.len() as i32) };
    if previous_mode != 0 {
        let _ = unsafe { SetPolyFillMode(hdc, previous_mode) };
    }
    if !previous_brush.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_brush) };
    }
    if !previous_pen.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_pen) };
    }
    let _ = unsafe { DeleteObject(brush) };
}

fn map_contour(contour: &[(f32, f32)], rect: RECT) -> Vec<POINT> {
    let width = (rect.right - rect.left).max(1) as f32;
    let height = (rect.bottom - rect.top).max(1) as f32;
    let mut mapped = Vec::with_capacity(contour.len());
    for &(x, y) in contour {
        let point = POINT {
            x: rect.left + ((x / VIEW) * width).round() as i32,
            y: rect.top + ((y / VIEW) * height).round() as i32,
        };
        if !same_point(mapped.last(), point) {
            mapped.push(point);
        }
    }
    if mapped.len() >= 2 && same_point(mapped.first(), mapped[mapped.len() - 1]) {
        mapped.pop();
    }
    mapped
}

fn flatten_svg_path(path: &str) -> Vec<Vec<(f32, f32)>> {
    let mut contours = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;
    let bytes = path.as_bytes();
    let mut x = 0.0_f32;
    let mut y = 0.0_f32;
    let mut start_x = 0.0_f32;
    let mut start_y = 0.0_f32;
    let mut last_control = None;
    let mut last_cmd = b'm';

    while i < bytes.len() {
        i = skip_sep(bytes, i);
        if i >= bytes.len() {
            break;
        }
        let cmd = if bytes[i].is_ascii_alphabetic() {
            let cmd = bytes[i];
            i += 1;
            cmd
        } else if last_cmd == b'm' {
            b'l'
        } else if last_cmd == b'M' {
            b'L'
        } else {
            last_cmd
        };
        let relative = cmd.is_ascii_lowercase();
        let kind = cmd.to_ascii_lowercase();
        let previous_cmd = last_cmd;
        last_cmd = cmd;

        if kind == b'z' {
            close_contour(&mut current, start_x, start_y, &mut contours);
            x = start_x;
            y = start_y;
            last_control = None;
            continue;
        }

        loop {
            i = skip_sep(bytes, i);
            let Some((nums, next)) = take_numbers(path, i, arity(kind)) else {
                break;
            };
            i = next;
            match kind {
                b'm' => {
                    close_contour(&mut current, start_x, start_y, &mut contours);
                    if relative {
                        x += nums[0];
                        y += nums[1];
                    } else {
                        x = nums[0];
                        y = nums[1];
                    }
                    start_x = x;
                    start_y = y;
                    current.push((x, y));
                    last_control = None;
                    last_cmd = if relative { b'l' } else { b'L' };
                }
                b'l' => {
                    if relative {
                        x += nums[0];
                        y += nums[1];
                    } else {
                        x = nums[0];
                        y = nums[1];
                    }
                    current.push((x, y));
                    last_control = None;
                }
                b'h' => {
                    if relative {
                        x += nums[0];
                    } else {
                        x = nums[0];
                    }
                    current.push((x, y));
                    last_control = None;
                }
                b'v' => {
                    if relative {
                        y += nums[0];
                    } else {
                        y = nums[0];
                    }
                    current.push((x, y));
                    last_control = None;
                }
                b'q' => {
                    let (cx, cy, ex, ey) = if relative {
                        (x + nums[0], y + nums[1], x + nums[2], y + nums[3])
                    } else {
                        (nums[0], nums[1], nums[2], nums[3])
                    };
                    flatten_quad(&mut current, (x, y), (cx, cy), (ex, ey));
                    last_control = Some((cx, cy));
                    x = ex;
                    y = ey;
                }
                b't' => {
                    let (cx, cy) = reflect_control(previous_cmd, last_control, x, y, b'q');
                    let (ex, ey) = if relative {
                        (x + nums[0], y + nums[1])
                    } else {
                        (nums[0], nums[1])
                    };
                    flatten_quad(&mut current, (x, y), (cx, cy), (ex, ey));
                    last_control = Some((cx, cy));
                    x = ex;
                    y = ey;
                }
                b'c' => {
                    let (c1x, c1y, c2x, c2y, ex, ey) = if relative {
                        (
                            x + nums[0],
                            y + nums[1],
                            x + nums[2],
                            y + nums[3],
                            x + nums[4],
                            y + nums[5],
                        )
                    } else {
                        (nums[0], nums[1], nums[2], nums[3], nums[4], nums[5])
                    };
                    flatten_cubic(&mut current, (x, y), (c1x, c1y), (c2x, c2y), (ex, ey));
                    last_control = Some((c2x, c2y));
                    x = ex;
                    y = ey;
                }
                b's' => {
                    let (c1x, c1y) = reflect_control(previous_cmd, last_control, x, y, b'c');
                    let (c2x, c2y, ex, ey) = if relative {
                        (x + nums[0], y + nums[1], x + nums[2], y + nums[3])
                    } else {
                        (nums[0], nums[1], nums[2], nums[3])
                    };
                    flatten_cubic(&mut current, (x, y), (c1x, c1y), (c2x, c2y), (ex, ey));
                    last_control = Some((c2x, c2y));
                    x = ex;
                    y = ey;
                }
                _ => {}
            }
            if kind == b'm' {
                break;
            }
        }
    }
    close_contour(&mut current, start_x, start_y, &mut contours);
    contours
}

fn arity(kind: u8) -> usize {
    match kind {
        b'h' | b'v' => 1,
        b'm' | b'l' | b't' => 2,
        b'q' | b's' => 4,
        b'c' => 6,
        _ => 0,
    }
}

fn close_contour(
    current: &mut Vec<(f32, f32)>,
    start_x: f32,
    start_y: f32,
    contours: &mut Vec<Vec<(f32, f32)>>,
) {
    if current.len() >= 3 {
        if current.last() != Some(&(start_x, start_y)) {
            current.push((start_x, start_y));
        }
        contours.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn reflect_control(
    last_cmd: u8,
    last_control: Option<(f32, f32)>,
    x: f32,
    y: f32,
    family: u8,
) -> (f32, f32) {
    let kind = last_cmd.to_ascii_lowercase();
    if matches!((family, kind), (b'c', b'c' | b's') | (b'q', b'q' | b't')) {
        if let Some((cx, cy)) = last_control {
            return (2.0 * x - cx, 2.0 * y - cy);
        }
    }
    (x, y)
}

fn flatten_quad(out: &mut Vec<(f32, f32)>, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) {
    for step in 1..=CURVE_STEPS {
        let t = step as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
            u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
        ));
    }
}

fn flatten_cubic(
    out: &mut Vec<(f32, f32)>,
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
) {
    for step in 1..=CURVE_STEPS {
        let t = step as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
            u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
        ));
    }
}

fn take_numbers(path: &str, start: usize, count: usize) -> Option<([f32; 6], usize)> {
    if count == 0 {
        return None;
    }
    let mut nums = [0.0_f32; 6];
    let mut i = start;
    for slot in nums.iter_mut().take(count) {
        i = skip_sep(path.as_bytes(), i);
        let (value, next) = parse_number(path, i)?;
        *slot = value;
        i = next;
    }
    Some((nums, i))
}

fn parse_number(path: &str, start: usize) -> Option<(f32, usize)> {
    let bytes = path.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    let mut i = start;
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
    }
    let mut seen_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        seen_digit = true;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            seen_digit = true;
            i += 1;
        }
    }
    if !seen_digit {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    path[start..i].parse().ok().map(|value| (value, i))
}

fn same_point(previous: Option<&POINT>, point: POINT) -> bool {
    previous.is_some_and(|prev| prev.x == point.x && prev.y == point.y)
}

fn skip_sep(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
        i += 1;
    }
    i
}

// 24×24 y-down paths copied from tsuyoshi-otake/otak-usage `src/brandIcons.ts`.
const OPENAI_SVG_PATH: &str = "M22.278 9.798Q23.153 7.228 21.769 4.913Q20.781 3.191 18.974 2.4Q17.167 1.609 15.247 2.033Q14.174 0.791 12.621 0.296Q11.068 -0.198 9.487 0.141Q7.878 0.48 6.678 1.553Q5.478 2.626 4.969 4.179Q2.344 4.744 0.988 7.087Q0 8.781 0.212 10.744Q0.424 12.706 1.722 14.202Q0.847 16.772 2.231 19.087Q3.219 20.809 5.026 21.6Q6.833 22.391 8.753 21.967Q10.56 24 13.271 24Q15.219 24 16.814 22.828Q18.409 21.656 19.031 19.793Q21.713 19.228 23.04 16.913Q24 15.191 23.788 13.228Q23.576 11.266 22.278 9.798ZM13.271 22.419Q11.633 22.419 10.391 21.402L10.532 21.289L15.304 18.551Q15.699 18.296 15.699 17.873V11.153L17.732 12.311Q17.76 12.311 17.76 12.339V17.929Q17.76 19.172 17.139 20.202Q16.518 21.233 15.515 21.826Q14.513 22.419 13.271 22.419ZM3.586 18.296Q2.767 16.913 3.049 15.304L3.219 15.36L7.991 18.127Q8.358 18.381 8.781 18.127L14.598 14.767V17.111Q14.598 17.139 14.569 17.139L9.741 19.962Q8.668 20.584 7.468 20.555Q6.268 20.527 5.238 19.948Q4.207 19.369 3.586 18.296ZM2.344 7.878Q3.162 6.466 4.687 5.929V11.576Q4.687 12.056 5.111 12.282L10.899 15.642L8.866 16.8Q8.838 16.828 8.809 16.8L3.981 14.033Q2.908 13.412 2.329 12.367Q1.751 11.322 1.736 10.136Q1.722 8.951 2.344 7.878ZM18.918 11.746 13.101 8.358 15.134 7.2Q15.162 7.172 15.191 7.2L20.019 9.967Q21.148 10.616 21.755 11.788Q22.362 12.96 22.249 14.259Q22.136 15.558 21.36 16.588Q20.584 17.619 19.341 18.099V12.424Q19.341 11.972 18.918 11.746ZM20.951 8.753 20.809 8.64 16.038 5.873Q15.642 5.619 15.247 5.873L9.402 9.233V6.889Q9.402 6.861 9.431 6.833L14.259 4.038Q15.388 3.388 16.701 3.445Q18.014 3.501 19.087 4.264Q20.16 4.998 20.654 6.198Q21.148 7.398 20.951 8.696ZM8.301 12.847 6.268 11.689Q6.24 11.689 6.24 11.661V6.071Q6.24 4.772 6.946 3.671Q7.652 2.569 8.838 1.976Q10.024 1.44 11.322 1.609Q12.621 1.779 13.609 2.598L13.468 2.711L8.696 5.449Q8.301 5.704 8.301 6.127ZM9.402 10.504 12 8.979 14.598 10.504V13.496L12 14.993L9.402 13.496Z";

const CLAUDE_SVG_PATH: &str = "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z";

#[cfg(test)]
mod tests {
    use super::{flatten_svg_path, CLAUDE_SVG_PATH, OPENAI_SVG_PATH, VIEW};

    #[test]
    fn component_brand_paths_flatten_inside_the_24_viewbox() {
        let claude = flatten_svg_path(CLAUDE_SVG_PATH);
        assert_eq!(claude.len(), 1);
        assert!(claude[0].len() >= 16);
        assert_bounds(&claude);

        let openai = flatten_svg_path(OPENAI_SVG_PATH);
        assert!(openai.len() >= 6);
        assert_bounds(&openai);
    }

    fn assert_bounds(contours: &[Vec<(f32, f32)>]) {
        for contour in contours {
            for &(x, y) in contour {
                assert!(x > -2.0 && x < VIEW + 2.0, "x={x}");
                assert!(y > -2.0 && y < VIEW + 2.0, "y={y}");
            }
        }
    }
}
