// gui/video_panel.rs — Video display with detection overlays

use super::app::CoachApp;
use super::colors;
use crate::detection::COCO_SPORTS_BALL;
use crate::metrics::coach::{team_display_rgb, TeamId};
use eframe::egui;

pub fn show(app: &mut CoachApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    if app.frames.is_empty() {
        // Empty state
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label(
                    egui::RichText::new("Football Coach PoC")
                        .heading()
                        .color(colors::ACCENT),
                );
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new("Load a video clip to get started")
                        .size(16.0)
                        .color(colors::TEXT_SECONDARY),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(
                        "1. Click 'Load Video' to open a match clip\n\
                                         2. Click 'Load ONNX Model' to select your YOLOv8 model\n\
                                         3. Click 'Run Analysis' to process",
                    )
                    .color(colors::TEXT_SECONDARY),
                );
            });
        });
        return;
    }

    // Display current frame
    if let Some(texture) = &app.frame_texture {
        let available = ui.available_size();
        let tex_size = texture.size_vec2();

        // Scale to fit while maintaining aspect ratio
        let scale = (available.x / tex_size.x)
            .min(available.y / tex_size.y)
            .min(1.0);
        let display_size = tex_size * scale;

        let response = ui.allocate_response(display_size, egui::Sense::click());
        let rect = response.rect;

        // Draw the frame
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Draw detection overlays
        if app.show_bboxes {
            draw_detections(app, ui, rect, tex_size);
        }

        // Handle corner selection for homography
        if app.editing_corners {
            draw_corners(app, ui, rect, tex_size);
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let rel_x = (pos.x - rect.left()) / rect.width() * tex_size.x;
                    let rel_y = (pos.y - rect.top()) / rect.height() * tex_size.y;
                    app.homography_calibration.image_points[app.selected_reference_point] =
                        Some(crate::pitch_mapping::Point2D {
                            x: rel_x as f64,
                            y: rel_y as f64,
                        });
                    app.selected_reference_point = (app.selected_reference_point + 1)
                        % app.homography_calibration.image_points.len();
                }
            }
        }

        // Frame info overlay
        let info_text = format!(
            "Frame {}/{} | Source {}/{} | {:.2}s",
            app.displayed_frame_position() + 1,
            app.displayed_frame_count(),
            app.current_frame_idx + 1,
            app.frames.len(),
            app.frames[app.current_frame_idx].timestamp_secs,
        );
        ui.painter().text(
            rect.left_top() + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            info_text,
            egui::FontId::monospace(12.0),
            egui::Color32::from_rgba_premultiplied(255, 255, 255, 200),
        );
    }
}

fn draw_detections(app: &CoachApp, ui: &egui::Ui, rect: egui::Rect, tex_size: egui::Vec2) {
    let frame_idx = match app.current_frame() {
        Some(frame) => frame.index,
        None => return,
    };

    // Try to use tracked objects first, fall back to raw detections
    if let Some(tracking) = &app.tracking {
        if let Some(ft) = tracking
            .frame_tracks
            .iter()
            .find(|ft| ft.frame_index == frame_idx)
        {
            let coach = app.metrics.as_ref().map(|m| &m.coach_metrics);
            let team_color_for = |team: TeamId| -> egui::Color32 {
                if let Some(c) = coach {
                    if let Some((r, g, b)) = team_display_rgb(team, c) {
                        return egui::Color32::from_rgb(r, g, b);
                    }
                }
                match team {
                    TeamId::TeamA => colors::PLAYER_TEAM_A,
                    TeamId::TeamB => colors::PLAYER_TEAM_B,
                }
            };

            for track in &ft.tracks {
                let color = if track.class_id == COCO_SPORTS_BALL {
                    colors::BALL_COLOR
                } else {
                    let team = coach
                        .and_then(|c| c.team_assignments.get(&track.track_id))
                        .copied();
                    match team {
                        Some(t) => team_color_for(t),
                        None => colors::TEXT_PRIMARY,
                    }
                };

                draw_bbox(
                    ui,
                    rect,
                    tex_size,
                    track.bbox.x1,
                    track.bbox.y1,
                    track.bbox.x2,
                    track.bbox.y2,
                    color,
                );

                if app.show_ids {
                    let label = format!("#{} {:.0}%", track.track_id, track.confidence * 100.0);
                    let screen_x = rect.left() + (track.bbox.x1 / tex_size.x) * rect.width();
                    let screen_y = rect.top() + (track.bbox.y1 / tex_size.y) * rect.height() - 14.0;

                    ui.painter().text(
                        egui::pos2(screen_x, screen_y),
                        egui::Align2::LEFT_BOTTOM,
                        label,
                        egui::FontId::monospace(10.0),
                        color,
                    );
                }

                // Draw movement trail
                if app.show_trails && track.class_name == "Player" {
                    draw_trail(app, ui, rect, tex_size, track.track_id, frame_idx);
                }
            }
            return;
        }
    }

    // Fallback: raw detections
    if let Some(fd) = app.detections.iter().find(|fd| fd.frame_index == frame_idx) {
        for det in &fd.detections {
            let color = if det.class_id == COCO_SPORTS_BALL {
                colors::BALL_COLOR
            } else {
                colors::PLAYER_TEAM_A
            };

            draw_bbox(
                ui,
                rect,
                tex_size,
                det.bbox.x1,
                det.bbox.y1,
                det.bbox.x2,
                det.bbox.y2,
                color,
            );
        }
    }
}

fn draw_bbox(
    ui: &egui::Ui,
    rect: egui::Rect,
    tex_size: egui::Vec2,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: egui::Color32,
) {
    let sx1 = rect.left() + (x1 / tex_size.x) * rect.width();
    let sy1 = rect.top() + (y1 / tex_size.y) * rect.height();
    let sx2 = rect.left() + (x2 / tex_size.x) * rect.width();
    let sy2 = rect.top() + (y2 / tex_size.y) * rect.height();

    ui.painter().rect_stroke(
        egui::Rect::from_min_max(egui::pos2(sx1, sy1), egui::pos2(sx2, sy2)),
        0.0,
        egui::Stroke::new(2.0, color),
        egui::StrokeKind::Outside,
    );
}

fn draw_trail(
    app: &CoachApp,
    ui: &egui::Ui,
    rect: egui::Rect,
    tex_size: egui::Vec2,
    track_id: u32,
    current_frame: u64,
) {
    let tracking = match &app.tracking {
        Some(t) => t,
        None => return,
    };

    // Collect recent positions for this track (last 30 frames)
    let trail_len = 30u64;
    let start_frame = current_frame.saturating_sub(trail_len);

    let positions: Vec<egui::Pos2> = tracking
        .frame_tracks
        .iter()
        .filter(|ft| ft.frame_index >= start_frame && ft.frame_index <= current_frame)
        .filter_map(|ft| {
            ft.tracks.iter().find(|t| t.track_id == track_id).map(|t| {
                let cx = (t.bbox.x1 + t.bbox.x2) / 2.0;
                let cy = (t.bbox.y1 + t.bbox.y2) / 2.0;
                egui::pos2(
                    rect.left() + (cx / tex_size.x) * rect.width(),
                    rect.top() + (cy / tex_size.y) * rect.height(),
                )
            })
        })
        .collect();

    // Draw trail as connected line segments with fading alpha
    for i in 1..positions.len() {
        let alpha = (i as f32 / positions.len() as f32 * 150.0) as u8;
        let color = egui::Color32::from_rgba_premultiplied(255, 255, 0, alpha);
        ui.painter().line_segment(
            [positions[i - 1], positions[i]],
            egui::Stroke::new(1.5, color),
        );
    }
}

fn draw_corners(app: &CoachApp, ui: &egui::Ui, rect: egui::Rect, tex_size: egui::Vec2) {
    for (i, point) in app.homography_calibration.image_points.iter().enumerate() {
        let Some(point) = point else {
            continue;
        };

        let sx = rect.left() + (point.x as f32 / tex_size.x) * rect.width();
        let sy = rect.top() + (point.y as f32 / tex_size.y) * rect.height();

        let color = if i == app.selected_reference_point {
            egui::Color32::from_rgb(255, 0, 255)
        } else {
            egui::Color32::from_rgb(255, 255, 0)
        };

        ui.painter().circle_filled(egui::pos2(sx, sy), 6.0, color);
        ui.painter().text(
            egui::pos2(sx + 10.0, sy - 10.0),
            egui::Align2::LEFT_BOTTOM,
            app.homography_calibration.reference_points[i].short_label(),
            egui::FontId::monospace(12.0),
            color,
        );
    }

    // Draw helper lines between the visible calibration points.
    let points: Vec<egui::Pos2> = app
        .homography_calibration
        .image_points
        .iter()
        .filter_map(|point| point.as_ref())
        .map(|point| {
            egui::pos2(
                rect.left() + (point.x as f32 / tex_size.x) * rect.width(),
                rect.top() + (point.y as f32 / tex_size.y) * rect.height(),
            )
        })
        .collect();

    for i in 1..points.len() {
        ui.painter().line_segment(
            [points[i - 1], points[i]],
            egui::Stroke::new(
                1.5,
                egui::Color32::from_rgba_premultiplied(255, 255, 0, 128),
            ),
        );
    }
}
