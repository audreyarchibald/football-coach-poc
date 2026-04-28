// video_processor/mod.rs — Video decoding via ffmpeg-next

pub mod decoder;

use anyhow::Result;
use image::RgbImage;
use std::path::Path;

/// Metadata about a loaded video
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub total_frames: u64,
    pub duration_secs: f64,
    pub path: String,
}

/// A single decoded video frame
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub index: u64,
    pub timestamp_secs: f64,
    pub image: RgbImage,
}

/// Load video info without decoding frames
pub fn probe_video(path: &Path) -> Result<VideoInfo> {
    decoder::probe(path)
}

/// Decode all frames from a video clip
pub fn decode_all_frames(
    path: &Path,
    max_frames: Option<u64>,
) -> Result<(VideoInfo, Vec<VideoFrame>)> {
    decoder::decode_frames(path, max_frames)
}

/// Decode frames at a specific interval (e.g., every Nth frame for analysis)
pub fn decode_sampled_frames(
    path: &Path,
    sample_every_n: u64,
) -> Result<(VideoInfo, Vec<VideoFrame>)> {
    decoder::decode_sampled(path, sample_every_n)
}
