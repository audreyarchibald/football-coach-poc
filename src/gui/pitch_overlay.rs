// gui/pitch_overlay.rs — Draw top-down pitch with player positions

use super::colors;
use crate::detection::COCO_SPORTS_BALL;
use crate::metrics::coach::TeamId;
use crate::pitch_mapping::{PITCH_LENGTH, PITCH_WIDTH};
use crate::tracker::FrameTracks;
use eframe::egui;

/// Draw a top-down pitch view with current player positions
pub fn draw_pitch_overlay(
    ui: &mut egui::Ui,
    frame_tracks: Option<&FrameTracks>,
    mapper: Option<&crate::pitch_mapping::PitchMapper>,
    team_assignments: Option<&std::collections::HashMap<u32, TeamId>>,
) {
    let available_width = ui.available_width().min(380.0);
    let pitch_aspect = PITCH_LENGTH as f32 / PITCH_WIDTH as f32;
    let width = available_width;
    let height = width / pitch_aspect;

    let (response, painter) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());

    let rect = response.rect;

    // Green pitch
    painter.rect_filled(rect, 4.0, colors::PITCH_GREEN);

    // Pitch lines (same as heatmap)
    draw_pitch_lines_full(&painter, rect);

    // Draw player positions
    if let (Some(ft), Some(mapper)) = (frame_tracks, mapper) {
        for track in &ft.tracks {
            let pitch_pos =
                mapper.bbox_to_pitch(track.bbox.x1, track.bbox.y1, track.bbox.x2, track.bbox.y2);

            // Map pitch coords to screen
            let sx = rect.left() + (pitch_pos.x as f32 / PITCH_LENGTH as f32) * width;
            let sy = rect.top() + (pitch_pos.y as f32 / PITCH_WIDTH as f32) * height;

            if track.class_id == COCO_SPORTS_BALL {
                painter.circle_filled(egui::pos2(sx, sy), 4.0, colors::BALL_COLOR);
            } else {
                let color = match team_assignments.and_then(|teams| teams.get(&track.track_id)) {
                    Some(TeamId::TeamA) => colors::PLAYER_TEAM_A,
                    Some(TeamId::TeamB) => colors::PLAYER_TEAM_B,
                    None => colors::TEXT_PRIMARY,
                };
                painter.circle_filled(egui::pos2(sx, sy), 6.0, color);
                painter.text(
                    egui::pos2(sx, sy - 10.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{}", track.track_id),
                    egui::FontId::monospace(9.0),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}

fn draw_pitch_lines_full(painter: &egui::Painter, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.5, colors::PITCH_LINE);
    let w = rect.width();
    let h = rect.height();

    // Outline
    painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Outside);

    // Center line
    painter.line_segment(
        [
            egui::pos2(rect.left() + w / 2.0, rect.top()),
            egui::pos2(rect.left() + w / 2.0, rect.bottom()),
        ],
        stroke,
    );

    // Center circle
    painter.circle_stroke(rect.center(), h * 0.15, stroke);
    painter.circle_filled(rect.center(), 2.5, colors::PITCH_LINE);

    // Penalty areas
    let pen_w = w * 16.5 / 105.0;
    let pen_h = h * 40.3 / 68.0;
    let pen_top = rect.top() + (h - pen_h) / 2.0;

    painter.rect_stroke(
        egui::Rect::from_min_size(egui::pos2(rect.left(), pen_top), egui::vec2(pen_w, pen_h)),
        0.0,
        stroke,
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        egui::Rect::from_min_size(
            egui::pos2(rect.right() - pen_w, pen_top),
            egui::vec2(pen_w, pen_h),
        ),
        0.0,
        stroke,
        egui::StrokeKind::Outside,
    );

    // Goal areas
    let goal_w = w * 5.5 / 105.0;
    let goal_h = h * 18.3 / 68.0;
    let goal_top = rect.top() + (h - goal_h) / 2.0;

    painter.rect_stroke(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), goal_top),
            egui::vec2(goal_w, goal_h),
        ),
        0.0,
        stroke,
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        egui::Rect::from_min_size(
            egui::pos2(rect.right() - goal_w, goal_top),
            egui::vec2(goal_w, goal_h),
        ),
        0.0,
        stroke,
        egui::StrokeKind::Outside,
    );

    // Penalty spots
    let spot_left = rect.left() + w * 11.0 / 105.0;
    let spot_right = rect.right() - w * 11.0 / 105.0;
    let spot_y = rect.top() + h / 2.0;

    painter.circle_filled(egui::pos2(spot_left, spot_y), 2.0, colors::PITCH_LINE);
    painter.circle_filled(egui::pos2(spot_right, spot_y), 2.0, colors::PITCH_LINE);
}
