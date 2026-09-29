//! **Screenshots** of the primary window — the UI over the world, exactly as
//! the user would see it — optionally with the boxes of some semantic nodes
//! drawn over it, so a failure can show where the locator's matches were.
//!
//! A windowless viewer's primary window is its off-screen one, so the same
//! request captures a headless run. Capture is asynchronous: a request returns
//! a [`ScreenshotTicket`], and the frame arrives a frame or two later in
//! [`Screenshots`], where [`take_screenshot`] collects it.

use std::collections::HashMap;
use std::io::Cursor;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::PrimaryWindow;
use sl_automation_proto::Bounds;

/// The colour the overlaid boxes are drawn in: opaque magenta, which nothing
/// in the viewer's skins uses.
pub const OVERLAY_COLOUR: [u8; 4] = [255, 0, 255, 255];

/// How thick an overlaid box's outline is, in physical pixels.
pub const OVERLAY_THICKNESS: u32 = 2;

/// Collects captured frames. Added by whoever installs automation.
#[derive(Debug, Default)]
pub struct ScreenshotProbePlugin;

impl Plugin for ScreenshotProbePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screenshots>();
    }
}

/// Names one requested screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScreenshotTicket(u64);

/// A captured frame: tightly packed 8-bit RGBA rows, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    /// The width in physical pixels.
    pub width: u32,
    /// The height in physical pixels.
    pub height: u32,
    /// The pixels, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Why a frame could not be captured or encoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScreenshotError {
    /// The window's texture is in a format this cannot read as RGBA.
    #[error("the window texture could not be read as RGBA: {0}")]
    Format(String),
    /// The PNG encoder refused the frame.
    #[error("the frame could not be encoded as PNG: {0}")]
    Encode(String),
}

impl CapturedFrame {
    /// The frame as a PNG file.
    ///
    /// # Errors
    ///
    /// [`ScreenshotError::Encode`] when the encoder refuses it — which, for a
    /// frame whose buffer matches its size, it does not.
    pub fn to_png(&self) -> Result<Vec<u8>, ScreenshotError> {
        let image = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .ok_or_else(|| {
                ScreenshotError::Encode("the pixel buffer does not match the size".to_owned())
            })?;
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| ScreenshotError::Encode(error.to_string()))?;
        Ok(png.into_inner())
    }

    /// Outline each of `boxes` — logical pixels, as the semantic model reports
    /// them — at `scale_factor` physical pixels per logical one, clipped to the
    /// frame.
    pub fn outline(&mut self, boxes: &[Bounds], scale_factor: f32) {
        for bounds in boxes {
            let left = physical(bounds.x, scale_factor);
            let top = physical(bounds.y, scale_factor);
            let right = physical(bounds.x + bounds.width, scale_factor);
            let bottom = physical(bounds.y + bounds.height, scale_factor);
            let thick = OVERLAY_THICKNESS;
            self.fill(left, top, right, top.saturating_add(thick));
            self.fill(left, bottom.saturating_sub(thick), right, bottom);
            self.fill(left, top, left.saturating_add(thick), bottom);
            self.fill(right.saturating_sub(thick), top, right, bottom);
        }
    }

    /// Paint the rectangle `[left, right) × [top, bottom)` in the overlay
    /// colour, clipped to the frame.
    fn fill(&mut self, left: u32, top: u32, right: u32, bottom: u32) {
        let right = right.min(self.width);
        let bottom = bottom.min(self.height);
        let row_bytes = usize::try_from(self.width).unwrap_or(0).saturating_mul(4);
        for y in top..bottom {
            let row = usize::try_from(y)
                .unwrap_or(usize::MAX)
                .saturating_mul(row_bytes);
            for x in left..right {
                let at =
                    row.saturating_add(usize::try_from(x).unwrap_or(usize::MAX).saturating_mul(4));
                if let Some(pixel) = self.rgba.get_mut(at..at.saturating_add(4)) {
                    pixel.copy_from_slice(&OVERLAY_COLOUR);
                }
            }
        }
    }

    /// The pixel at `(x, y)`, when inside the frame.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = u64::from(y)
            .saturating_mul(u64::from(self.width))
            .saturating_add(u64::from(x))
            .saturating_mul(4);
        let at = usize::try_from(index).ok()?;
        self.rgba.get(at..at.saturating_add(4))?.try_into().ok()
    }
}

/// A logical coordinate in physical pixels, rounded and clamped to
/// `0..=65_535` — larger than any frame.
fn physical(logical: f32, scale_factor: f32) -> u32 {
    let scaled = (logical * scale_factor).round();
    if !scaled.is_finite() || scaled <= 0.0 {
        return 0;
    }
    let clamped = scaled.min(65_535.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped finite and in 0.0..=65_535.0, so it fits u32 exactly"
    )]
    let pixels = clamped as u32;
    pixels
}

/// Frames captured for requested tickets, until taken.
#[derive(Debug, Default, Resource)]
pub struct Screenshots {
    /// The next ticket to hand out.
    next: u64,
    /// Captured frames (or why one failed), by ticket.
    captured: HashMap<ScreenshotTicket, Result<CapturedFrame, ScreenshotError>>,
}

/// Ask for a screenshot of the primary window, with `overlay`'s boxes (logical
/// pixels) outlined on it. The frame lands in [`Screenshots`] once rendered;
/// collect it with [`take_screenshot`].
pub fn request_screenshot(world: &mut World, overlay: Vec<Bounds>) -> ScreenshotTicket {
    let scale_factor = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1.0, Window::scale_factor);
    let mut screenshots = world.get_resource_or_init::<Screenshots>();
    let ticket = ScreenshotTicket(screenshots.next);
    screenshots.next = screenshots.next.saturating_add(1);
    world.spawn(Screenshot::primary_window()).observe(
        move |shot: On<ScreenshotCaptured>, mut screenshots: ResMut<Screenshots>| {
            let frame = rgba_frame(&shot.image).map(|mut frame| {
                frame.outline(&overlay, scale_factor);
                frame
            });
            let _previous = screenshots.captured.insert(ticket, frame);
        },
    );
    ticket
}

/// The frame captured for `ticket`, once it has been — removed from
/// [`Screenshots`]; `None` while it is still being rendered.
pub fn take_screenshot(
    world: &mut World,
    ticket: ScreenshotTicket,
) -> Option<Result<CapturedFrame, ScreenshotError>> {
    world
        .get_resource_mut::<Screenshots>()?
        .captured
        .remove(&ticket)
}

/// A captured window texture as tightly packed RGBA.
fn rgba_frame(image: &Image) -> Result<CapturedFrame, ScreenshotError> {
    let rgba = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| ScreenshotError::Format(error.to_string()))?
        .to_rgba8();
    Ok(CapturedFrame {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_automation_proto::Bounds;

    use super::{CapturedFrame, OVERLAY_COLOUR};

    /// A black frame of `width` × `height`.
    fn black(width: u32, height: u32) -> CapturedFrame {
        let pixels = usize::try_from(width.saturating_mul(height)).unwrap_or(0);
        CapturedFrame {
            width,
            height,
            rgba: [0, 0, 0, 255].repeat(pixels),
        }
    }

    #[test]
    fn a_box_is_outlined_at_the_scale_factor_and_clipped() {
        let mut frame = black(40, 30);
        frame.outline(
            &[Bounds {
                x: 5.0,
                y: 5.0,
                width: 10.0,
                height: 5.0,
            }],
            2.0,
        );
        // The outline spans physical 10..30 × 10..20.
        assert_eq!(frame.pixel(10, 10), Some(OVERLAY_COLOUR), "top left");
        assert_eq!(frame.pixel(29, 19), Some(OVERLAY_COLOUR), "bottom right");
        assert_eq!(frame.pixel(20, 15), Some([0, 0, 0, 255]), "inside stays");
        assert_eq!(frame.pixel(9, 10), Some([0, 0, 0, 255]), "outside stays");
        let mut clipped = black(8, 8);
        clipped.outline(
            &[Bounds {
                x: -4.0,
                y: 4.0,
                width: 100.0,
                height: 100.0,
            }],
            1.0,
        );
        assert_eq!(clipped.pixel(0, 5), Some(OVERLAY_COLOUR));
        assert_eq!(clipped.pixel(7, 4), Some(OVERLAY_COLOUR));
    }

    #[test]
    fn a_frame_encodes_as_png() -> Result<(), super::ScreenshotError> {
        let png = black(3, 2).to_png()?;
        assert_eq!(png.get(1..4), Some(b"PNG".as_slice()));
        Ok(())
    }
}
