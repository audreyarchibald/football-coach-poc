// detection/yolo.rs — YOLOv8/v11 ONNX inference via ort crate

use super::{football_class_name, is_football_relevant, BBox, Detection, FrameDetections};
use crate::video_processor::VideoFrame;
use anyhow::Result;
use image::imageops::FilterType;
use image::RgbImage;
use log::{debug, info};
use ndarray::{Array, ArrayView};
use ort::session::Session;
use std::path::Path;

/// YOLO detector wrapper
pub struct YoloDetector {
    session: Session,
    input_size: u32, // typically 640
    conf_threshold: f32,
    iou_threshold: f32,
}

impl YoloDetector {
    /// Create a new YOLO detector from an ONNX model file
    pub fn new(model_path: &Path, conf_threshold: f32, iou_threshold: f32) -> Result<Self> {
        info!("Loading YOLO model from: {}", model_path.display());

        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Determine input size from model metadata
        let input_size = 640u32; // Standard YOLO input

        info!(
            "YOLO model loaded successfully (input size: {}x{})",
            input_size, input_size
        );

        Ok(Self {
            session,
            input_size,
            conf_threshold,
            iou_threshold,
        })
    }

    /// Preprocess an RGB image for YOLO input
    fn preprocess(&self, img: &RgbImage) -> Array<f32, ndarray::Ix4> {
        let resized =
            image::imageops::resize(img, self.input_size, self.input_size, FilterType::Triangle);

        let mut input = Array::zeros((1, 3, self.input_size as usize, self.input_size as usize));

        for y in 0..self.input_size as usize {
            for x in 0..self.input_size as usize {
                let pixel = resized.get_pixel(x as u32, y as u32);
                input[[0, 0, y, x]] = pixel[0] as f32 / 255.0;
                input[[0, 1, y, x]] = pixel[1] as f32 / 255.0;
                input[[0, 2, y, x]] = pixel[2] as f32 / 255.0;
            }
        }

        input
    }

    /// Run detection on a single frame
    pub fn detect_frame(&mut self, frame: &VideoFrame) -> Result<FrameDetections> {
        let input_tensor = self.preprocess(&frame.image);

        let input_value =
            ort::value::Value::from_array(input_tensor).map_err(|e| anyhow::anyhow!("{e}"))?;

        let outputs = self
            .session
            .run(ort::inputs!["images" => input_value])
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // YOLOv8 output shape: [1, 84, 8400] (for COCO 80 classes)
        // Extract output and convert to owned array to release the session borrow
        let output = outputs
            .values()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No output from YOLO model"))?;

        let output_array = output
            .try_extract_array::<f32>()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .into_owned();

        // Drop outputs to release the mutable borrow on session
        drop(outputs);

        let output_view = output_array.view();
        let detections = self.postprocess(
            &output_view,
            frame.image.width() as f32,
            frame.image.height() as f32,
        );

        Ok(FrameDetections {
            frame_index: frame.index,
            timestamp_secs: frame.timestamp_secs,
            detections,
        })
    }

    /// Post-process YOLO output: extract boxes, apply NMS
    fn postprocess(
        &self,
        output: &ArrayView<f32, ndarray::IxDyn>,
        orig_width: f32,
        orig_height: f32,
    ) -> Vec<Detection> {
        let mut raw_detections: Vec<Detection> = Vec::new();

        // YOLOv8 output: [1, 84, N] where N=8400 for 640x640
        // 84 = 4 (box) + 80 (class scores)
        let shape = output.shape();
        let (num_features, num_predictions) = if shape.len() == 3 {
            (shape[1], shape[2])
        } else {
            return raw_detections;
        };

        let scale_x = orig_width / self.input_size as f32;
        let scale_y = orig_height / self.input_size as f32;

        for i in 0..num_predictions {
            // Extract box coordinates (cx, cy, w, h)
            let cx = output[[0, 0, i]];
            let cy = output[[0, 1, i]];
            let w = output[[0, 2, i]];
            let h = output[[0, 3, i]];

            // Find best class
            let mut best_class = 0u32;
            let mut best_score = 0.0f32;

            for c in 4..num_features {
                let score = output[[0, c, i]];
                if score > best_score {
                    best_score = score;
                    best_class = (c - 4) as u32;
                }
            }

            if best_score < self.conf_threshold {
                continue;
            }

            // Only keep football-relevant detections
            if !is_football_relevant(best_class) {
                continue;
            }

            if !cx.is_finite() || !cy.is_finite() || !w.is_finite() || !h.is_finite() {
                continue;
            }

            // Convert from center format to corner format and scale
            let x1 = (cx - w / 2.0) * scale_x;
            let y1 = (cy - h / 2.0) * scale_y;
            let x2 = (cx + w / 2.0) * scale_x;
            let y2 = (cy + h / 2.0) * scale_y;

            if !x1.is_finite() || !y1.is_finite() || !x2.is_finite() || !y2.is_finite() {
                continue;
            }

            let bbox = BBox {
                x1: x1.max(0.0),
                y1: y1.max(0.0),
                x2: x2.min(orig_width),
                y2: y2.min(orig_height),
            };
            if !bbox.is_finite() {
                continue;
            }

            raw_detections.push(Detection {
                bbox,
                confidence: best_score,
                class_id: best_class,
                class_name: football_class_name(best_class).to_string(),
            });
        }

        // Apply NMS
        self.nms(&mut raw_detections);

        debug!("Post-NMS: {} detections", raw_detections.len());
        raw_detections
    }

    /// Greedy Non-Maximum Suppression
    fn nms(&self, detections: &mut Vec<Detection>) {
        detections
            .retain(|detection| detection.confidence.is_finite() && detection.bbox.is_finite());
        detections.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));

        let mut keep = vec![true; detections.len()];

        for i in 0..detections.len() {
            if !keep[i] {
                continue;
            }
            for j in (i + 1)..detections.len() {
                if !keep[j] {
                    continue;
                }
                if detections[i].class_id == detections[j].class_id
                    && detections[i].bbox.iou(&detections[j].bbox) > self.iou_threshold
                {
                    keep[j] = false;
                }
            }
        }

        let mut idx = 0;
        detections.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
    }

    /// Run detection on multiple frames (batch processing)
    pub fn detect_frames<F>(
        &mut self,
        frames: &[VideoFrame],
        mut on_progress: F,
    ) -> Result<Vec<FrameDetections>>
    where
        F: FnMut(f32),
    {
        info!("Running YOLO detection on {} frames...", frames.len());
        let mut results = Vec::with_capacity(frames.len());

        for (i, frame) in frames.iter().enumerate() {
            let dets = self.detect_frame(frame)?;
            on_progress((i + 1) as f32 / frames.len().max(1) as f32);
            if i % 30 == 0 {
                debug!(
                    "Processed frame {}/{} — {} detections",
                    i + 1,
                    frames.len(),
                    dets.detections.len()
                );
            }
            results.push(dets);
        }

        info!("Detection complete: {} frames processed", results.len());
        Ok(results)
    }
}
