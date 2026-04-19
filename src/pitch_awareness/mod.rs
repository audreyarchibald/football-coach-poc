// pitch_awareness/mod.rs — Automatic pitch and field-area inference

use crate::pitch_mapping::{HomographyCalibration, Point2D, PITCH_LENGTH, PITCH_WIDTH};
use crate::video_processor::VideoFrame;
use image::RgbImage;
use serde::{Deserialize, Serialize};

const MIN_TACTICAL_PLAYER_COUNT: usize = 8;
const MIN_TACTICAL_SPREAD_X: f32 = 0.35;
const MIN_TACTICAL_SPREAD_Y: f32 = 0.22;

/// Automatically inferred scene understanding for a clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAwareness {
    pub frame_width: u32,
    pub frame_height: u32,
    pub field_mask_ratio: f32,
    pub line_ratio: f32,
    pub confidence: f32,
    pub visible_width_ratio: f32,
    pub visible_height_ratio: f32,
    pub touchline_top_y: f32,
    pub touchline_bottom_y: f32,
    pub left_edge_top_x: f32,
    pub left_edge_bottom_x: f32,
    pub right_edge_top_x: f32,
    pub right_edge_bottom_x: f32,
    pub center_line_x: f32,
    pub goal_side_hint: GoalSideHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevantSegment {
    pub start_idx: usize,
    pub end_idx: usize,
    pub start_secs: f64,
    pub end_secs: f64,
    pub avg_confidence: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitchTrimSuggestion {
    pub segments: Vec<RelevantSegment>,
    pub representative_scene: Option<SceneAwareness>,
    pub kept_frames: usize,
    pub kept_duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRelevance {
    pub frame_index: u64,
    pub timestamp_secs: f64,
    pub score: f32,
    pub keep: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalSideHint {
    Left,
    Right,
    Unknown,
}

impl std::fmt::Display for GoalSideHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalSideHint::Left => write!(f, "Left"),
            GoalSideHint::Right => write!(f, "Right"),
            GoalSideHint::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Infer pitch geometry from a broadcast frame using grass segmentation and white-line cues.
pub fn infer_scene_awareness(frame: &VideoFrame) -> Option<SceneAwareness> {
    let image = &frame.image;
    let width = image.width() as usize;
    let height = image.height() as usize;
    if width < 64 || height < 64 {
        return None;
    }

    let row_green_counts: Vec<usize> = (0..height)
        .map(|y| {
            (0..width)
                .filter(|&x| is_pitch_green(image.get_pixel(x as u32, y as u32).0))
                .count()
        })
        .collect();

    let min_green_per_row = (width as f32 * 0.18) as usize;
    let top_y = row_green_counts
        .iter()
        .position(|&count| count >= min_green_per_row)?;
    let bottom_y = row_green_counts
        .iter()
        .rposition(|&count| count >= min_green_per_row)?;
    if bottom_y <= top_y + height / 8 {
        return None;
    }

    let sample_rows = [
        top_y,
        top_y + (bottom_y - top_y) / 4,
        top_y + (bottom_y - top_y) / 2,
        top_y + (bottom_y - top_y) * 3 / 4,
        bottom_y,
    ];

    let mut left_edges = Vec::new();
    let mut right_edges = Vec::new();
    let mut green_pixels = 0usize;
    let mut line_pixels = 0usize;

    for &row in &sample_rows {
        let (left, right, count, row_line_pixels) = row_edges(image, row)?;
        left_edges.push(left);
        right_edges.push(right);
        green_pixels += count;
        line_pixels += row_line_pixels;
    }

    let left_edge_top_x = average(&left_edges[..2]);
    let left_edge_bottom_x = average(&left_edges[left_edges.len() - 2..]);
    let right_edge_top_x = average(&right_edges[..2]);
    let right_edge_bottom_x = average(&right_edges[right_edges.len() - 2..]);
    let center_line_x = detect_center_line_x(
        image,
        top_y,
        bottom_y,
        left_edge_bottom_x,
        right_edge_bottom_x,
    )
    .unwrap_or((left_edge_bottom_x + right_edge_bottom_x) / 2.0);

    let field_height = (bottom_y - top_y + 1) as f32;
    let field_width_top = (right_edge_top_x - left_edge_top_x).max(1.0);
    let field_width_bottom = (right_edge_bottom_x - left_edge_bottom_x).max(1.0);
    let field_mask_ratio = green_pixels as f32 / (width * sample_rows.len()) as f32;
    let line_ratio = line_pixels as f32 / (green_pixels.max(1)) as f32;
    let perspective_ratio = (field_width_bottom / field_width_top).clamp(0.0, 4.0);
    let perspective_score = if perspective_ratio > 0.65 && perspective_ratio < 3.2 {
        1.0
    } else {
        0.4
    };
    let confidence =
        (field_mask_ratio * 0.55 + (line_ratio * 8.0).min(0.3) + perspective_score * 0.15)
            .clamp(0.0, 1.0);
    let goal_side_hint = infer_goal_side(
        left_edge_bottom_x,
        right_edge_bottom_x,
        field_width_bottom,
        width as f32,
    );

    if field_height < height as f32 * 0.2 || field_width_bottom < width as f32 * 0.25 {
        return None;
    }

    Some(SceneAwareness {
        frame_width: width as u32,
        frame_height: height as u32,
        field_mask_ratio,
        line_ratio,
        confidence,
        visible_width_ratio: (field_width_bottom / width as f32).clamp(0.0, 1.0),
        visible_height_ratio: (field_height / height as f32).clamp(0.0, 1.0),
        touchline_top_y: top_y as f32,
        touchline_bottom_y: bottom_y as f32,
        left_edge_top_x,
        left_edge_bottom_x,
        right_edge_top_x,
        right_edge_bottom_x,
        center_line_x,
        goal_side_hint,
    })
}

/// Convert inferred pitch geometry into an automatic homography calibration.
pub fn auto_calibration_from_scene(scene: &SceneAwareness) -> HomographyCalibration {
    let image_points = [
        Point2D {
            x: scene.left_edge_top_x as f64,
            y: scene.touchline_top_y as f64,
        },
        Point2D {
            x: scene.right_edge_top_x as f64,
            y: scene.touchline_top_y as f64,
        },
        Point2D {
            x: scene.right_edge_bottom_x as f64,
            y: scene.touchline_bottom_y as f64,
        },
        Point2D {
            x: scene.left_edge_bottom_x as f64,
            y: scene.touchline_bottom_y as f64,
        },
    ];

    let visible_width_ratio = ((scene.right_edge_bottom_x - scene.left_edge_bottom_x)
        / (scene.right_edge_top_x - scene.left_edge_top_x).max(1.0))
    .clamp(0.0, 3.0);
    let pitch_top_inset = ((visible_width_ratio - 1.0).max(0.0) * 10.0).min(18.0) as f64;

    let pitch_points = [
        Point2D {
            x: pitch_top_inset,
            y: 0.0,
        },
        Point2D {
            x: PITCH_LENGTH - pitch_top_inset,
            y: 0.0,
        },
        Point2D {
            x: PITCH_LENGTH,
            y: PITCH_WIDTH,
        },
        Point2D {
            x: 0.0,
            y: PITCH_WIDTH,
        },
    ];

    HomographyCalibration::from_auto_detected(image_points, pitch_points)
}

/// Detect contiguous pitch-visible ranges that are worth keeping.
pub fn detect_relevant_segments(
    frames: &[VideoFrame],
    sample_every_n: usize,
    min_confidence: f32,
    min_duration_secs: f64,
    max_gap_frames: usize,
) -> PitchTrimSuggestion {
    if frames.is_empty() {
        return PitchTrimSuggestion {
            segments: Vec::new(),
            representative_scene: None,
            kept_frames: 0,
            kept_duration_secs: 0.0,
        };
    }

    let step = sample_every_n.max(1);
    let mut samples = Vec::new();
    let mut best_scene = None;
    let mut best_confidence = 0.0f32;

    for idx in (0..frames.len()).step_by(step) {
        let scene = infer_scene_awareness(&frames[idx]);
        let confidence = scene.as_ref().map(tactical_view_score).unwrap_or(0.0);
        if confidence > best_confidence {
            best_confidence = confidence;
            best_scene = scene.clone();
        }
        samples.push((idx, frames[idx].timestamp_secs, confidence));
    }

    let mut segments = Vec::new();
    let mut current_start = None;
    let mut current_end = 0usize;
    let mut current_conf_sum = 0.0f32;
    let mut current_conf_count = 0usize;
    let mut last_relevant_idx = None;

    for (idx, _, confidence) in samples {
        let relevant = confidence >= min_confidence;

        if relevant {
            if current_start.is_none() {
                current_start = Some(idx);
                current_conf_sum = 0.0;
                current_conf_count = 0;
            } else if let Some(last_idx) = last_relevant_idx {
                if idx.saturating_sub(last_idx) > max_gap_frames {
                    push_segment(
                        frames,
                        &mut segments,
                        current_start.take(),
                        current_end,
                        current_conf_sum,
                        current_conf_count,
                        min_duration_secs,
                    );
                    current_start = Some(idx);
                    current_conf_sum = 0.0;
                    current_conf_count = 0;
                }
            }

            current_end = (idx + step - 1).min(frames.len() - 1);
            current_conf_sum += confidence;
            current_conf_count += 1;
            last_relevant_idx = Some(idx);
        }
    }

    push_segment(
        frames,
        &mut segments,
        current_start,
        current_end,
        current_conf_sum,
        current_conf_count,
        min_duration_secs,
    );

    merge_close_segments(frames, &mut segments, max_gap_frames.max(step));

    let kept_frames = segments
        .iter()
        .map(|segment| segment.end_idx - segment.start_idx + 1)
        .sum();
    let kept_duration_secs = segments
        .iter()
        .map(|segment| (segment.end_secs - segment.start_secs).max(0.0))
        .sum();

    PitchTrimSuggestion {
        segments,
        representative_scene: best_scene,
        kept_frames,
        kept_duration_secs,
    }
}

/// Keep only frames that still look like live pitch views.
pub fn filter_pitch_frames(
    frames: &[VideoFrame],
    reference_scene: &SceneAwareness,
    min_confidence: f32,
) -> Vec<VideoFrame> {
    frames
        .iter()
        .filter_map(|frame| {
            let scene = infer_scene_awareness(frame)?;
            let same_goal_side = matches!(
                (reference_scene.goal_side_hint, scene.goal_side_hint),
                (GoalSideHint::Unknown, _)
                    | (_, GoalSideHint::Unknown)
                    | (GoalSideHint::Left, GoalSideHint::Left)
                    | (GoalSideHint::Right, GoalSideHint::Right)
            );
            let compatible_shape =
                (scene.field_mask_ratio - reference_scene.field_mask_ratio).abs() < 0.35;

            if tactical_view_score(&scene) >= min_confidence && same_goal_side && compatible_shape {
                Some(frame.clone())
            } else {
                None
            }
        })
        .collect()
}

pub fn build_detection_sampling_plan(
    frames: &[VideoFrame],
    sample_every_n: usize,
) -> Vec<VideoFrame> {
    if frames.is_empty() {
        return Vec::new();
    }

    let step = sample_every_n.max(1);
    frames
        .iter()
        .enumerate()
        .filter(|(idx, _)| idx % step == 0)
        .map(|(_, frame)| frame.clone())
        .collect()
}

pub fn expand_sampled_frame_indices(
    sampled_indices: &[usize],
    full_len: usize,
    radius: usize,
) -> Vec<usize> {
    let mut keep = vec![false; full_len];
    for &idx in sampled_indices {
        let start = idx.saturating_sub(radius);
        let end = (idx + radius).min(full_len.saturating_sub(1));
        for slot in keep.iter_mut().take(end + 1).skip(start) {
            *slot = true;
        }
    }

    keep.into_iter()
        .enumerate()
        .filter_map(|(idx, keep)| keep.then_some(idx))
        .collect()
}

pub fn score_tactical_wide_shot(
    frame: &VideoFrame,
    scene: Option<&SceneAwareness>,
    detections: Option<&crate::detection::FrameDetections>,
) -> f32 {
    let mut score = scene.map(|scene| scene.confidence * 0.45).unwrap_or(0.0);

    if let Some(detections) = detections {
        let player_boxes: Vec<_> = detections
            .detections
            .iter()
            .filter(|d| d.class_id == crate::detection::COCO_PERSON)
            .collect();

        let player_count = player_boxes.len();
        if player_count >= MIN_TACTICAL_PLAYER_COUNT {
            score += 0.3;
        } else {
            score -= 0.15;
        }

        if let Some(spread) =
            player_spread_score(&player_boxes, frame.image.width(), frame.image.height())
        {
            score += spread * 0.35;
        }

        let closeup_penalty =
            closeup_penalty(&player_boxes, frame.image.width(), frame.image.height());
        score -= closeup_penalty;
    }

    score.clamp(0.0, 1.0)
}

pub fn is_tactical_wide_shot(
    frame: &VideoFrame,
    scene: Option<&SceneAwareness>,
    detections: Option<&crate::detection::FrameDetections>,
    threshold: f32,
) -> bool {
    score_tactical_wide_shot(frame, scene, detections) >= threshold
}

pub fn tactical_view_score(scene: &SceneAwareness) -> f32 {
    let mut score = scene.confidence;

    if scene.visible_width_ratio < 0.55 {
        score -= 0.22;
    } else if scene.visible_width_ratio > 0.72 {
        score += 0.08;
    }

    if scene.visible_height_ratio < 0.30 {
        score -= 0.18;
    } else if scene.visible_height_ratio > 0.42 {
        score += 0.06;
    }

    if scene.line_ratio < 0.008 {
        score -= 0.10;
    } else if scene.line_ratio > 0.025 {
        score += 0.05;
    }

    score.clamp(0.0, 1.0)
}

fn push_segment(
    frames: &[VideoFrame],
    segments: &mut Vec<RelevantSegment>,
    start_idx: Option<usize>,
    end_idx: usize,
    conf_sum: f32,
    conf_count: usize,
    min_duration_secs: f64,
) {
    let Some(start_idx) = start_idx else {
        return;
    };
    if end_idx < start_idx || conf_count == 0 {
        return;
    }

    let start_secs = frames[start_idx].timestamp_secs;
    let end_secs = frames[end_idx].timestamp_secs;
    if end_secs - start_secs < min_duration_secs {
        return;
    }

    segments.push(RelevantSegment {
        start_idx,
        end_idx,
        start_secs,
        end_secs,
        avg_confidence: conf_sum / conf_count as f32,
        enabled: true,
    });
}

fn merge_close_segments(
    frames: &[VideoFrame],
    segments: &mut Vec<RelevantSegment>,
    max_gap_frames: usize,
) {
    if segments.len() < 2 {
        return;
    }

    let mut merged = Vec::with_capacity(segments.len());
    let mut current = segments[0].clone();

    for segment in segments.iter().skip(1) {
        if segment.start_idx <= current.end_idx + max_gap_frames {
            let current_len = current.end_idx - current.start_idx + 1;
            let next_len = segment.end_idx - segment.start_idx + 1;
            let total_len = current_len + next_len;
            current.end_idx = segment.end_idx;
            current.end_secs = frames[current.end_idx].timestamp_secs;
            current.avg_confidence = (current.avg_confidence * current_len as f32
                + segment.avg_confidence * next_len as f32)
                / total_len as f32;
        } else {
            merged.push(current);
            current = segment.clone();
        }
    }

    merged.push(current);
    *segments = merged;
}

fn row_edges(image: &RgbImage, row: usize) -> Option<(f32, f32, usize, usize)> {
    let width = image.width() as usize;
    let mut left = None;
    let mut right = None;
    let mut green_count = 0usize;
    let mut line_count = 0usize;

    for x in 0..width {
        let pixel = image.get_pixel(x as u32, row as u32).0;
        if is_pitch_green(pixel) {
            green_count += 1;
            left.get_or_insert(x as f32);
            right = Some(x as f32);
        }
        if is_pitch_line(pixel) {
            line_count += 1;
        }
    }

    Some((left?, right?, green_count, line_count))
}

fn detect_center_line_x(
    image: &RgbImage,
    top_y: usize,
    bottom_y: usize,
    left_x: f32,
    right_x: f32,
) -> Option<f32> {
    let width = image.width() as usize;
    let scan_left = left_x.max(0.0) as usize;
    let scan_right = right_x.min((width.saturating_sub(1)) as f32) as usize;
    if scan_right <= scan_left {
        return None;
    }

    let mut best_x = None;
    let mut best_score = 0usize;
    let mid_y = top_y + (bottom_y - top_y) / 2;
    let scan_band_top = mid_y.saturating_sub((bottom_y - top_y) / 6);
    let scan_band_bottom = (mid_y + (bottom_y - top_y) / 6).min(bottom_y);

    for x in scan_left..=scan_right {
        let mut score = 0usize;
        for y in scan_band_top..=scan_band_bottom {
            if is_pitch_line(image.get_pixel(x as u32, y as u32).0) {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            best_x = Some(x as f32);
        }
    }

    (best_score > (scan_band_bottom - scan_band_top) / 8)
        .then_some(best_x?)
        .or(best_x)
}

fn player_spread_score(
    player_boxes: &[&crate::detection::Detection],
    width: u32,
    height: u32,
) -> Option<f32> {
    if player_boxes.len() < 2 {
        return None;
    }

    let mut min_x = f32::MAX;
    let mut max_x: f32 = 0.0;
    let mut min_y = f32::MAX;
    let mut max_y: f32 = 0.0;

    for det in player_boxes {
        let (cx, cy) = det.bbox.center();
        min_x = min_x.min(cx);
        max_x = max_x.max(cx);
        min_y = min_y.min(cy);
        max_y = max_y.max(cy);
    }

    let spread_x = ((max_x - min_x) / width as f32).clamp(0.0, 1.0);
    let spread_y = ((max_y - min_y) / height as f32).clamp(0.0, 1.0);

    let x_score = (spread_x / MIN_TACTICAL_SPREAD_X).clamp(0.0, 1.0);
    let y_score = (spread_y / MIN_TACTICAL_SPREAD_Y).clamp(0.0, 1.0);
    Some((x_score * 0.65 + y_score * 0.35).clamp(0.0, 1.0))
}

fn closeup_penalty(player_boxes: &[&crate::detection::Detection], width: u32, height: u32) -> f32 {
    let frame_area = (width * height) as f32;
    let large_players = player_boxes
        .iter()
        .filter(|det| det.bbox.area() / frame_area > 0.08)
        .count();
    let total_player_area: f32 = player_boxes.iter().map(|det| det.bbox.area()).sum();
    let area_ratio = (total_player_area / frame_area).clamp(0.0, 1.0);

    let mut penalty = 0.0;
    if large_players >= 2 {
        penalty += 0.2;
    }
    if area_ratio > 0.28 {
        penalty += 0.2;
    }
    penalty
}

fn is_pitch_green(pixel: [u8; 3]) -> bool {
    let r = pixel[0] as i32;
    let g = pixel[1] as i32;
    let b = pixel[2] as i32;
    g > 60 && g > r + 12 && g > b + 8
}

fn is_pitch_line(pixel: [u8; 3]) -> bool {
    let r = pixel[0] as i32;
    let g = pixel[1] as i32;
    let b = pixel[2] as i32;
    r > 165 && g > 165 && b > 165 && (r - g).abs() < 28 && (g - b).abs() < 28
}

fn average(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn infer_goal_side(left_x: f32, right_x: f32, field_width: f32, frame_width: f32) -> GoalSideHint {
    let left_margin = left_x.max(0.0);
    let right_margin = (frame_width - right_x).max(0.0);
    let threshold = field_width * 0.18;

    if left_margin > right_margin + threshold {
        GoalSideHint::Left
    } else if right_margin > left_margin + threshold {
        GoalSideHint::Right
    } else {
        GoalSideHint::Unknown
    }
}
