// gui/heatmap_view.rs — Render heatmap overlaid on a football pitch

use super::colors;
use crate::metrics::heatmap::{HeatmapData, GRID_COLS, GRID_ROWS};
use eframe::egui;

/// Draw a heatmap on a mini pitch diagram
pub fn draw_heatmap(ui: &mut egui::Ui, data: &HeatmapData, player_id: Option<u32>) {
    let available_width = ui.available_width().min(380.0);
    let pitch_aspect = 105.0 / 68.0;
    let width = available_width;
    let height = width / pitch_aspect;

    let (response, painter) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());

    let rect = response.rect;

    // Draw pitch background
    painter.rect_filled(rect, 4.0, colors::PITCH_GREEN);

    // Draw pitch lines
    draw_pitch_lines(&painter, rect);

    // Draw heatmap cells
    let cell_w = width / GRID_COLS as f32;
    let cell_h = height / GRID_ROWS as f32;

    let grid = if let Some(pid) = player_id {
        data.per_player.get(&pid)
    } else {
        Some(&data.grid)
    };

    if let Some(grid) = grid {
        // Find local max for normalization
        let local_max = grid
            .iter()
            .flat_map(|row| row.iter())
            .cloned()
            .fold(0.0f32, f32::max);

        if local_max > 0.0 {
            for row in 0..GRID_ROWS {
                for col in 0..GRID_COLS {
                    let value = grid[row][col] / local_max;
                    if value > 0.01 {
                        let color = colors::heatmap_color(value);
                        let alpha = (value * 180.0) as u8;
                        let color = egui::Color32::from_rgba_premultiplied(
                            color.r(),
                            color.g(),
                            color.b(),
                            alpha,
                        );

                        let cell_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                rect.left() + col as f32 * cell_w,
                                rect.top() + row as f32 * cell_h,
                            ),
                            egui::vec2(cell_w, cell_h),
                        );

                        painter.rect_filled(cell_rect, 0.0, color);
                    }
                }
            }
        }
    }

    // Tooltip with value
    if let Some(hover_pos) = response.hover_pos() {
        let col = ((hover_pos.x - rect.left()) / cell_w) as usize;
        let row = ((hover_pos.y - rect.top()) / cell_h) as usize;
        if row < GRID_ROWS && col < GRID_COLS {
            let val = if let Some(pid) = player_id {
                data.per_player
                    .get(&pid)
                    .map(|g| g[row][col])
                    .unwrap_or(0.0)
            } else {
                data.grid[row][col]
            };
            egui::show_tooltip(
                ui.ctx(),
                response.layer_id,
                egui::Id::new("heatmap_tooltip"),
                |ui| {
                    ui.label(format!("Zone [{},{}]: {:.0} samples", row, col, val));
                },
            );
        }
    }
}

/// Draw simplified pitch markings
fn draw_pitch_lines(painter: &egui::Painter, rect: egui::Rect) {
    let stroke = egui::Stroke::new(
        1.0,
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 100),
    );
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
    let center = rect.center();
    painter.circle_stroke(center, h * 0.15, stroke);

    // Center spot
    painter.circle_filled(
        center,
        2.0,
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 100),
    );

    // Penalty areas (simplified proportions)
    let pen_w = w * 16.5 / 105.0;
    let pen_h = h * 40.3 / 68.0;
    let pen_top = rect.top() + (h - pen_h) / 2.0;

    // Left penalty area
    painter.rect_stroke(
        egui::Rect::from_min_size(egui::pos2(rect.left(), pen_top), egui::vec2(pen_w, pen_h)),
        0.0,
        stroke,
        egui::StrokeKind::Outside,
    );

    // Right penalty area
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
}
