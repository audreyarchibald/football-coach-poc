// metrics/heatmap.rs — 2D pitch heatmap generation

use super::TimedPosition;
use crate::pitch_mapping::{PITCH_LENGTH, PITCH_WIDTH};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Grid resolution for the heatmap
pub const GRID_COLS: usize = 21; // ~5m per cell
pub const GRID_ROWS: usize = 14; // ~5m per cell

/// Heatmap data: occupancy counts per grid cell
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatmapData {
    /// Grid[row][col] = count of position samples
    pub grid: Vec<Vec<f32>>,
    /// Per-player heatmaps (track_id -> grid)
    pub per_player: HashMap<u32, Vec<Vec<f32>>>,
    /// Max value in the combined grid (for normalization)
    pub max_value: f32,
    pub cols: usize,
    pub rows: usize,
}

impl HeatmapData {
    pub fn empty() -> Self {
        Self {
            grid: vec![vec![0.0; GRID_COLS]; GRID_ROWS],
            per_player: HashMap::new(),
            max_value: 0.0,
            cols: GRID_COLS,
            rows: GRID_ROWS,
        }
    }

    /// Get normalized value [0.0, 1.0] at (row, col)
    pub fn normalized(&self, row: usize, col: usize) -> f32 {
        if self.max_value > 0.0 {
            self.grid[row][col] / self.max_value
        } else {
            0.0
        }
    }
}

/// Compute a heatmap from player positions
pub fn compute_heatmap(player_positions: &HashMap<u32, Vec<TimedPosition>>) -> HeatmapData {
    let mut data = HeatmapData::empty();
    let cell_w = PITCH_LENGTH / GRID_COLS as f64;
    let cell_h = PITCH_WIDTH / GRID_ROWS as f64;

    for (&track_id, positions) in player_positions {
        let mut player_grid = vec![vec![0.0f32; GRID_COLS]; GRID_ROWS];

        for pos in positions {
            let col = ((pos.pitch_pos.x / cell_w) as usize).min(GRID_COLS - 1);
            let row = ((pos.pitch_pos.y / cell_h) as usize).min(GRID_ROWS - 1);

            data.grid[row][col] += 1.0;
            player_grid[row][col] += 1.0;
        }

        data.per_player.insert(track_id, player_grid);
    }

    // Find max
    data.max_value = data
        .grid
        .iter()
        .flat_map(|row| row.iter())
        .cloned()
        .fold(0.0f32, f32::max);

    log::info!(
        "Heatmap computed: {}x{} grid, max_value={:.1}",
        GRID_COLS,
        GRID_ROWS,
        data.max_value
    );

    data
}
