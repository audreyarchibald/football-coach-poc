use crate::video_processor::{VideoFrame, VideoInfo};
use anyhow::Result;
use image::imageops::FilterType;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    Display(u32),
    Window(u32),
}

#[derive(Debug, Clone)]
pub struct CaptureSource {
    pub target: CaptureTarget,
    pub label: String,
}

pub struct ScreenCapture {
    frame_index: u64,
    started_at: Instant,
    target: CaptureTarget,
    max_width: u32,
}

impl ScreenCapture {
    pub fn new(target: CaptureTarget, max_width: u32) -> Self {
        Self {
            frame_index: 0,
            started_at: Instant::now(),
            target,
            max_width,
        }
    }

    pub fn available_sources() -> Result<Vec<CaptureSource>> {
        #[cfg(target_os = "macos")]
        {
            macos::available_sources()
        }

        #[cfg(not(target_os = "macos"))]
        {
            bail!("Live window capture is currently implemented only for macOS")
        }
    }

    pub fn capture_frame(&mut self) -> Result<(VideoInfo, VideoFrame)> {
        #[cfg(target_os = "macos")]
        {
            let mut image = macos::capture_target(&self.target)?;

            if image.width() > self.max_width {
                let scaled_height = (image.height() as f32
                    * (self.max_width as f32 / image.width() as f32))
                    .round() as u32;
                image = image::imageops::resize(
                    &image,
                    self.max_width,
                    scaled_height.max(1),
                    FilterType::Triangle,
                );
            }

            let timestamp_secs = self.started_at.elapsed().as_secs_f64();
            let frame = VideoFrame {
                index: self.frame_index,
                timestamp_secs,
                image: image.clone(),
            };
            self.frame_index += 1;

            let info = VideoInfo {
                width: image.width(),
                height: image.height(),
                fps: 0.0,
                total_frames: self.frame_index,
                duration_secs: timestamp_secs,
                path: match self.target {
                    CaptureTarget::Display(id) => format!("live-display-{id}"),
                    CaptureTarget::Window(id) => format!("live-window-{id}"),
                },
            };

            Ok((info, frame))
        }

        #[cfg(not(target_os = "macos"))]
        {
            bail!("Live window capture is currently implemented only for macOS")
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{CaptureSource, CaptureTarget};
    use anyhow::{anyhow, bail, Context, Result};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_graphics::display::CGDisplay;
    use core_graphics::display::CGRectInfinite;
    use core_graphics::image::CGImageRef;
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowImageBestResolution, kCGWindowImageBoundsIgnoreFraming,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
        kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    };
    use image::RgbImage;

    pub fn available_sources() -> Result<Vec<CaptureSource>> {
        let mut sources = Vec::new();

        sources.push(CaptureSource {
            target: CaptureTarget::Display(1),
            label: "Main Display".to_string(),
        });

        let window_list = core_graphics::window::copy_window_info(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        )
        .ok_or_else(|| anyhow!("failed to enumerate macOS windows"))?;

        for idx in 0..window_list.len() {
            let Some(raw_entry) = window_list.get(idx) else {
                continue;
            };
            let Some(entry) =
                unsafe { CFType::wrap_under_get_rule(*raw_entry) }.downcast::<CFDictionary>()
            else {
                continue;
            };

            let window_id = cf_number_value(&entry, unsafe { kCGWindowNumber as _ });
            let owner = cf_string_value(&entry, unsafe { kCGWindowOwnerName as _ })
                .unwrap_or_else(|| "Unknown App".to_string());
            let title = cf_string_value(&entry, unsafe { kCGWindowName as _ }).unwrap_or_default();

            let Some(window_id) = window_id else {
                continue;
            };

            if owner == "Window Server" || (owner == "Dock" && title.is_empty()) {
                continue;
            }

            let trimmed_title = title.trim();
            let label = if trimmed_title.is_empty() {
                owner.clone()
            } else {
                format!("{} - {}", owner, trimmed_title)
            };

            sources.push(CaptureSource {
                target: CaptureTarget::Window(window_id as u32),
                label,
            });
        }

        Ok(sources)
    }

    pub fn capture_target(target: &CaptureTarget) -> Result<RgbImage> {
        let cg_image = match target {
            CaptureTarget::Display(display_id) => CGDisplay::new(*display_id).image(),
            CaptureTarget::Window(window_id) => CGDisplay::screenshot(
                unsafe { CGRectInfinite },
                kCGWindowListOptionIncludingWindow,
                *window_id,
                kCGWindowImageBoundsIgnoreFraming | kCGWindowImageBestResolution,
            ),
        }
        .ok_or_else(|| anyhow!("failed to capture requested display/window"))?;

        cg_image_to_rgb(&cg_image)
    }

    fn cf_number_value(dict: &CFDictionary, key: *const std::ffi::c_void) -> Option<i64> {
        dict.find(key)
            .and_then(|value| unsafe { CFType::wrap_under_get_rule(*value) }.downcast::<CFNumber>())
            .and_then(|number| number.to_i64())
    }

    fn cf_string_value(dict: &CFDictionary, key: *const std::ffi::c_void) -> Option<String> {
        dict.find(key)
            .and_then(|value| {
                unsafe { CFType::wrap_under_get_rule(*value) }
                    .downcast::<core_foundation::string::CFString>()
            })
            .map(|s| s.to_string())
    }

    fn cg_image_to_rgb(image: &CGImageRef) -> Result<RgbImage> {
        let width = image.width();
        let height = image.height();
        let bytes_per_row = image.bytes_per_row();
        let data = image.data();
        let bytes = data.bytes();

        if width == 0 || height == 0 {
            bail!("captured image was empty")
        }

        let mut rgb = vec![0u8; width * height * 3];
        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let src = row_start + x * 4;
                let dst = (y * width + x) * 3;

                if src + 2 >= bytes.len() || dst + 2 >= rgb.len() {
                    continue;
                }

                rgb[dst] = bytes[src + 2];
                rgb[dst + 1] = bytes[src + 1];
                rgb[dst + 2] = bytes[src];
            }
        }

        RgbImage::from_raw(width as u32, height as u32, rgb)
            .context("failed to build RGB image from captured frame")
    }
}
