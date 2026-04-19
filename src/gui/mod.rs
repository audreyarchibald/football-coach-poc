// gui/mod.rs — egui application module

pub mod analysis_panel;
pub mod app;
pub mod heatmap_view;
pub mod pitch_overlay;
pub mod video_panel;

/// Color palette for the football UI
pub mod colors {
    use egui::Color32;

    pub const PITCH_GREEN: Color32 = Color32::from_rgb(34, 139, 34);
    pub const PITCH_GREEN_LIGHT: Color32 = Color32::from_rgb(50, 160, 50);
    pub const PITCH_LINE: Color32 = Color32::from_rgb(255, 255, 255);

    pub const PLAYER_TEAM_A: Color32 = Color32::from_rgb(0, 100, 255);
    pub const PLAYER_TEAM_B: Color32 = Color32::from_rgb(255, 60, 60);
    pub const BALL_COLOR: Color32 = Color32::from_rgb(255, 215, 0);
    pub const TRACK_TRAIL: Color32 = Color32::from_rgba_premultiplied(255, 255, 0, 100);

    pub const BG_DARK: Color32 = Color32::from_rgb(24, 26, 32);
    pub const PANEL_BG: Color32 = Color32::from_rgb(32, 34, 42);
    pub const ACCENT: Color32 = Color32::from_rgb(0, 180, 120);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(230, 230, 230);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 160, 170);

    pub const HEATMAP_COLD: Color32 = Color32::from_rgb(0, 0, 180);
    pub const HEATMAP_WARM: Color32 = Color32::from_rgb(255, 255, 0);
    pub const HEATMAP_HOT: Color32 = Color32::from_rgb(255, 0, 0);

    /// Interpolate heatmap color based on value [0.0, 1.0]
    pub fn heatmap_color(t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            let s = t * 2.0;
            Color32::from_rgb(
                (HEATMAP_COLD.r() as f32 * (1.0 - s) + HEATMAP_WARM.r() as f32 * s) as u8,
                (HEATMAP_COLD.g() as f32 * (1.0 - s) + HEATMAP_WARM.g() as f32 * s) as u8,
                (HEATMAP_COLD.b() as f32 * (1.0 - s) + HEATMAP_WARM.b() as f32 * s) as u8,
            )
        } else {
            let s = (t - 0.5) * 2.0;
            Color32::from_rgb(
                (HEATMAP_WARM.r() as f32 * (1.0 - s) + HEATMAP_HOT.r() as f32 * s) as u8,
                (HEATMAP_WARM.g() as f32 * (1.0 - s) + HEATMAP_HOT.g() as f32 * s) as u8,
                (HEATMAP_WARM.b() as f32 * (1.0 - s) + HEATMAP_HOT.b() as f32 * s) as u8,
            )
        }
    }
}
