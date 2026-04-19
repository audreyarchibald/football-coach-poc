// detection/mod.rs — YOLO object detection via ONNX Runtime

pub mod yolo;

use serde::{Deserialize, Serialize};

/// A single detection from YOLO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    /// Bounding box: (x_min, y_min, x_max, y_max) in pixel coordinates
    pub bbox: BBox,
    /// Confidence score [0.0, 1.0]
    pub confidence: f32,
    /// Class ID from COCO/custom model
    pub class_id: u32,
    /// Human-readable class name
    pub class_name: String,
}

/// Axis-aligned bounding box
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BBox {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl BBox {
    pub fn width(&self) -> f32 {
        self.x2 - self.x1
    }
    pub fn height(&self) -> f32 {
        self.y2 - self.y1
    }
    pub fn center(&self) -> (f32, f32) {
        ((self.x1 + self.x2) / 2.0, (self.y1 + self.y2) / 2.0)
    }
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }
    pub fn iou(&self, other: &BBox) -> f32 {
        let inter_x1 = self.x1.max(other.x1);
        let inter_y1 = self.y1.max(other.y1);
        let inter_x2 = self.x2.min(other.x2);
        let inter_y2 = self.y2.min(other.y2);

        let inter_area = (inter_x2 - inter_x1).max(0.0) * (inter_y2 - inter_y1).max(0.0);
        let union_area = self.area() + other.area() - inter_area;

        if union_area > 0.0 {
            inter_area / union_area
        } else {
            0.0
        }
    }

    pub fn is_finite(&self) -> bool {
        self.x1.is_finite() && self.y1.is_finite() && self.x2.is_finite() && self.y2.is_finite()
    }
}

/// Detections for a single frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDetections {
    pub frame_index: u64,
    pub timestamp_secs: f64,
    pub detections: Vec<Detection>,
}

/// COCO class IDs relevant to football
pub const COCO_PERSON: u32 = 0;
pub const COCO_SPORTS_BALL: u32 = 32;

/// Check if a detection is football-relevant
pub fn is_football_relevant(class_id: u32) -> bool {
    matches!(class_id, COCO_PERSON | COCO_SPORTS_BALL)
}

/// Get display name for football-relevant classes
pub fn football_class_name(class_id: u32) -> &'static str {
    match class_id {
        COCO_PERSON => "Player",
        COCO_SPORTS_BALL => "Ball",
        _ => "Unknown",
    }
}
