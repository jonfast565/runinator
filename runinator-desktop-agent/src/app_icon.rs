//! The Runinator mark used by the desktop agent's native window and tray integration.

use std::io::Cursor;

use eframe::egui::IconData;
use tray_icon::Icon;

const RUNINATOR_ICON: &[u8] =
    include_bytes!("../../runinator-command-center/src-tauri/icons/icon.png");
const IDLE_ICON: &[u8] = include_bytes!("../assets/tray/idle.png");
const CONNECTING_ICON: &[u8] = include_bytes!("../assets/tray/connecting.png");
const CONNECTED_ICON: &[u8] = include_bytes!("../assets/tray/connected.png");
const RECONNECTING_ICON: &[u8] = include_bytes!("../assets/tray/reconnecting.png");
const DISCONNECTED_ICON: &[u8] = include_bytes!("../assets/tray/disconnected.png");

/// The full-background tray icon that maps to an agent connection state.
///
/// Amber and red distinguish a transient retry from a state that needs operator attention.
#[derive(Debug, Clone, Copy)]
pub enum TrayColor {
    /// Stopped or not started — neutral gray.
    Idle,
    /// Bringing the worker loop up — blue.
    Connecting,
    /// Running and consuming actions — green.
    Connected,
    /// Broker down, retrying within its budget — amber.
    Reconnecting,
    /// Disconnected for good, or re-enrollment is required — red.
    Disconnected,
}

impl TrayColor {
    fn image(self) -> &'static [u8] {
        match self {
            Self::Idle => IDLE_ICON,
            Self::Connecting => CONNECTING_ICON,
            Self::Connected => CONNECTED_ICON,
            Self::Reconnecting => RECONNECTING_ICON,
            Self::Disconnected => DISCONNECTED_ICON,
        }
    }
}

/// Load the shared high-resolution application icon as RGBA pixels.
///
/// The source asset is also included by the command-center bundles and the macOS runtime-app
/// packager, so every shipped native surface presents the same Runinator mark.
fn decode_rgba(image: &[u8]) -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(Cursor::new(image));
    let mut reader = decoder
        .read_info()
        .expect("the bundled Runinator icon must be a valid PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut pixels)
        .expect("the bundled Runinator icon must decode");
    pixels.truncate(info.buffer_size());

    assert_eq!(info.bit_depth, png::BitDepth::Eight);

    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        color_type => panic!("unsupported Runinator icon color type: {color_type:?}"),
    };

    (rgba, info.width, info.height)
}

pub fn window_icon() -> IconData {
    let (rgba, width, height) = decode_rgba(RUNINATOR_ICON);
    IconData {
        rgba,
        width,
        height,
    }
}

pub fn tray_icon(status: TrayColor) -> Icon {
    let (rgba, width, height) = status_rgba(status);
    Icon::from_rgba(rgba, width, height).expect("the bundled Runinator icon must be valid RGBA")
}

/// Decode a status asset and apply the canonical icon's alpha mask. The generated color variants
/// are RGB PNGs, while the shared app icon defines the transparent rounded corners native tray
/// surfaces expect.
fn status_rgba(status: TrayColor) -> (Vec<u8>, u32, u32) {
    let (mut rgba, width, height) = decode_rgba(status.image());
    let (mask, mask_width, mask_height) = decode_rgba(RUNINATOR_ICON);
    assert_eq!((width, height), (mask_width, mask_height));

    for (pixel, mask_pixel) in rgba.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
        pixel[3] = mask_pixel[3];
    }

    (rgba, width, height)
}

#[cfg(test)]
mod tests {
    use super::{TrayColor, status_rgba};

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn status_icons_have_distinct_backgrounds_and_transparent_corners() {
        let mut samples = Vec::new();
        for status in [
            TrayColor::Idle,
            TrayColor::Connecting,
            TrayColor::Connected,
            TrayColor::Reconnecting,
            TrayColor::Disconnected,
        ] {
            let (rgba, width, height) = status_rgba(status);
            assert_eq!((width, height), (512, 512));
            assert_eq!(pixel(&rgba, width, 0, 0)[3], 0);
            assert_eq!(pixel(&rgba, width, width / 2, height / 2)[3], 255);
            samples.push(pixel(&rgba, width, 80, 80));
        }

        samples.sort_unstable();
        samples.dedup();
        assert_eq!(samples.len(), 5);
    }
}
