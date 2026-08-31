//! The Runinator mark used by the desktop agent's native window and tray integration.

use std::io::Cursor;

use eframe::egui::IconData;
use tray_icon::Icon;

const RUNINATOR_ICON: &[u8] =
    include_bytes!("../../runinator-command-center/src-tauri/icons/icon.png");

/// Load the shared high-resolution application icon as RGBA pixels.
///
/// The source asset is also included by the command-center bundles and the macOS runtime-app
/// packager, so every shipped native surface presents the same Runinator mark.
fn rgba() -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(Cursor::new(RUNINATOR_ICON));
    let mut reader = decoder
        .read_info()
        .expect("the bundled Runinator icon must be a valid PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut pixels)
        .expect("the bundled Runinator icon must decode");
    pixels.truncate(info.buffer_size());

    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);

    (pixels, info.width, info.height)
}

pub fn window_icon() -> IconData {
    let (rgba, width, height) = rgba();
    IconData {
        rgba,
        width,
        height,
    }
}

pub fn tray_icon(status_color: [u8; 3]) -> Icon {
    let (mut rgba, width, height) = rgba();
    add_status_badge(&mut rgba, width, height, status_color);
    Icon::from_rgba(rgba, width, height).expect("the bundled Runinator icon must be valid RGBA")
}

/// Overlay a compact, high-contrast status badge in the open lower-right corner of the app icon.
/// The badge makes the current connection state visible without replacing the Runinator mark.
fn add_status_badge(rgba: &mut [u8], width: u32, height: u32, color: [u8; 3]) {
    let radius = (width.min(height) / 8) as i32;
    let center_x = width as i32 - radius - 2;
    let center_y = height as i32 - radius - 2;
    let border_radius_squared = radius * radius;
    let fill_radius = (radius - radius / 4).max(1);
    let fill_radius_squared = fill_radius * fill_radius;

    for y in (center_y - radius)..=(center_y + radius) {
        for x in (center_x - radius)..=(center_x + radius) {
            let dx = x - center_x;
            let dy = y - center_y;
            let distance_squared = dx * dx + dy * dy;
            if distance_squared > border_radius_squared {
                continue;
            }

            let index = ((y as u32 * width + x as u32) * 4) as usize;
            let pixel = if distance_squared <= fill_radius_squared {
                color
            } else {
                [5, 33, 92]
            };
            rgba[index..index + 3].copy_from_slice(&pixel);
            rgba[index + 3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::add_status_badge;

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn status_badge_preserves_the_brand_icon_and_exposes_its_color() {
        let mut rgba = vec![255; 32 * 32 * 4];
        add_status_badge(&mut rgba, 32, 32, [64, 180, 96]);

        assert_eq!(pixel(&rgba, 32, 26, 26), [64, 180, 96, 255]);
        assert_eq!(pixel(&rgba, 32, 22, 26), [5, 33, 92, 255]);
        assert_eq!(pixel(&rgba, 32, 0, 0), [255, 255, 255, 255]);
    }
}
