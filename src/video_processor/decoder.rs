// video_processor/decoder.rs — ffmpeg-based video decoder

use super::{VideoFrame, VideoInfo};
use anyhow::{Context, Result};
use image::RgbImage;
use log::{debug, info};
use std::path::Path;

/// Probe video metadata without full decode
pub fn probe(path: &Path) -> Result<VideoInfo> {
    let ictx = ffmpeg_next::format::input(path)
        .with_context(|| format!("Failed to open video: {}", path.display()))?;

    let stream = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

    let codec = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
    let decoder = codec.decoder().video()?;

    let fps = f64::from(stream.avg_frame_rate());
    let duration_secs = ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE);
    let total_frames = (duration_secs * fps).ceil() as u64;

    Ok(VideoInfo {
        width: decoder.width(),
        height: decoder.height(),
        fps,
        total_frames,
        duration_secs,
        path: path.to_string_lossy().to_string(),
    })
}

/// Decode all (or up to max_frames) frames from the video
pub fn decode_frames(path: &Path, max_frames: Option<u64>) -> Result<(VideoInfo, Vec<VideoFrame>)> {
    let info = probe(path)?;
    info!(
        "Decoding video: {}x{} @ {:.1} fps, {:.1}s",
        info.width, info.height, info.fps, info.duration_secs
    );

    let mut ictx = ffmpeg_next::format::input(path)?;

    // Extract stream info in a scoped block (streams() borrows ictx)
    let (video_stream_index, time_base, codec_params) = {
        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream"))?;
        (
            stream.index(),
            f64::from(stream.time_base()),
            stream.parameters(),
        )
    };

    let context_decoder = ffmpeg_next::codec::context::Context::from_parameters(codec_params)?;
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = ffmpeg_next::software::scaling::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg_next::format::Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        ffmpeg_next::software::scaling::Flags::BILINEAR,
    )?;

    let mut frames = Vec::new();
    let mut frame_index: u64 = 0;
    let max = max_frames.unwrap_or(u64::MAX);

    let receive_and_process = |decoder: &mut ffmpeg_next::decoder::Video,
                               scaler: &mut ffmpeg_next::software::scaling::Context,
                               frames: &mut Vec<VideoFrame>,
                               frame_index: &mut u64|
     -> Result<bool> {
        let mut decoded = ffmpeg_next::util::frame::video::Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            if *frame_index >= max {
                return Ok(true);
            }

            let mut rgb_frame = ffmpeg_next::util::frame::video::Video::empty();
            scaler.run(&decoded, &mut rgb_frame)?;

            let width = rgb_frame.width();
            let height = rgb_frame.height();
            let stride = rgb_frame.stride(0);
            let data = rgb_frame.data(0);

            // Copy row by row to handle stride != width*3
            let mut pixels = Vec::with_capacity((width * height * 3) as usize);
            for y in 0..height as usize {
                let row_start = y * stride;
                let row_end = row_start + (width as usize * 3);
                pixels.extend_from_slice(&data[row_start..row_end]);
            }

            let img = RgbImage::from_raw(width, height, pixels)
                .ok_or_else(|| anyhow::anyhow!("Failed to create image from frame data"))?;

            let timestamp = decoded.pts().unwrap_or(0) as f64 * time_base;

            frames.push(VideoFrame {
                index: *frame_index,
                timestamp_secs: timestamp,
                image: img,
            });

            if *frame_index % 100 == 0 {
                debug!("Decoded frame {}", frame_index);
            }
            *frame_index += 1;
        }
        Ok(false)
    };

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            if receive_and_process(&mut decoder, &mut scaler, &mut frames, &mut frame_index)? {
                break;
            }
        }
    }

    // Flush decoder
    decoder.send_eof()?;
    let _ = receive_and_process(&mut decoder, &mut scaler, &mut frames, &mut frame_index);

    info!("Decoded {} frames total", frames.len());
    Ok((info, frames))
}

/// Decode every Nth frame (for faster analysis)
pub fn decode_sampled(path: &Path, sample_every_n: u64) -> Result<(VideoInfo, Vec<VideoFrame>)> {
    let (info, all_frames) = decode_frames(path, None)?;
    let sampled: Vec<VideoFrame> = all_frames
        .into_iter()
        .filter(|f| f.index % sample_every_n == 0)
        .collect();
    info!(
        "Sampled {} frames (every {}th)",
        sampled.len(),
        sample_every_n
    );
    Ok((info, sampled))
}
