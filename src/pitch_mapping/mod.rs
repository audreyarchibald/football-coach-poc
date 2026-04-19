// pitch_mapping/mod.rs — Homography for 2D pitch projection

use anyhow::Result;
use log::info;
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Standard football pitch dimensions in meters (FIFA)
pub const PITCH_LENGTH: f64 = 105.0;
pub const PITCH_WIDTH: f64 = 68.0;
pub const CENTER_CIRCLE_RADIUS: f64 = 9.15;
pub const PENALTY_AREA_DEPTH: f64 = 16.5;
pub const PENALTY_AREA_WIDTH: f64 = 40.32;
pub const PENALTY_SPOT_DISTANCE: f64 = 11.0;

/// A point in 2D space
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

/// How the current pitch mapping was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationMode {
    Manual,
    Automatic,
}

impl std::fmt::Display for CalibrationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalibrationMode::Manual => write!(f, "Manual"),
            CalibrationMode::Automatic => write!(f, "Automatic"),
        }
    }
}

/// Named pitch landmarks that can be used for manual homography calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchReferencePoint {
    TopLeftCorner,
    TopRightCorner,
    BottomRightCorner,
    BottomLeftCorner,
    CenterSpot,
    CenterLineTop,
    CenterLineBottom,
    CenterCircleTop,
    CenterCircleBottom,
    LeftPenaltySpot,
    RightPenaltySpot,
    LeftPenaltyAreaTopRight,
    LeftPenaltyAreaBottomRight,
    RightPenaltyAreaTopLeft,
    RightPenaltyAreaBottomLeft,
}

impl PitchReferencePoint {
    pub fn all() -> &'static [Self] {
        const ALL: [PitchReferencePoint; 15] = [
            PitchReferencePoint::TopLeftCorner,
            PitchReferencePoint::TopRightCorner,
            PitchReferencePoint::BottomRightCorner,
            PitchReferencePoint::BottomLeftCorner,
            PitchReferencePoint::CenterSpot,
            PitchReferencePoint::CenterLineTop,
            PitchReferencePoint::CenterLineBottom,
            PitchReferencePoint::CenterCircleTop,
            PitchReferencePoint::CenterCircleBottom,
            PitchReferencePoint::LeftPenaltySpot,
            PitchReferencePoint::RightPenaltySpot,
            PitchReferencePoint::LeftPenaltyAreaTopRight,
            PitchReferencePoint::LeftPenaltyAreaBottomRight,
            PitchReferencePoint::RightPenaltyAreaTopLeft,
            PitchReferencePoint::RightPenaltyAreaBottomLeft,
        ];
        &ALL
    }

    pub fn label(&self) -> &'static str {
        match self {
            PitchReferencePoint::TopLeftCorner => "Top-left corner",
            PitchReferencePoint::TopRightCorner => "Top-right corner",
            PitchReferencePoint::BottomRightCorner => "Bottom-right corner",
            PitchReferencePoint::BottomLeftCorner => "Bottom-left corner",
            PitchReferencePoint::CenterSpot => "Center spot",
            PitchReferencePoint::CenterLineTop => "Halfway line top touchline",
            PitchReferencePoint::CenterLineBottom => "Halfway line bottom touchline",
            PitchReferencePoint::CenterCircleTop => "Center circle top",
            PitchReferencePoint::CenterCircleBottom => "Center circle bottom",
            PitchReferencePoint::LeftPenaltySpot => "Left penalty spot",
            PitchReferencePoint::RightPenaltySpot => "Right penalty spot",
            PitchReferencePoint::LeftPenaltyAreaTopRight => "Left penalty box top-right",
            PitchReferencePoint::LeftPenaltyAreaBottomRight => "Left penalty box bottom-right",
            PitchReferencePoint::RightPenaltyAreaTopLeft => "Right penalty box top-left",
            PitchReferencePoint::RightPenaltyAreaBottomLeft => "Right penalty box bottom-left",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            PitchReferencePoint::TopLeftCorner => "TL",
            PitchReferencePoint::TopRightCorner => "TR",
            PitchReferencePoint::BottomRightCorner => "BR",
            PitchReferencePoint::BottomLeftCorner => "BL",
            PitchReferencePoint::CenterSpot => "C",
            PitchReferencePoint::CenterLineTop => "HL-T",
            PitchReferencePoint::CenterLineBottom => "HL-B",
            PitchReferencePoint::CenterCircleTop => "CC-T",
            PitchReferencePoint::CenterCircleBottom => "CC-B",
            PitchReferencePoint::LeftPenaltySpot => "L-PS",
            PitchReferencePoint::RightPenaltySpot => "R-PS",
            PitchReferencePoint::LeftPenaltyAreaTopRight => "L-PB-T",
            PitchReferencePoint::LeftPenaltyAreaBottomRight => "L-PB-B",
            PitchReferencePoint::RightPenaltyAreaTopLeft => "R-PB-T",
            PitchReferencePoint::RightPenaltyAreaBottomLeft => "R-PB-B",
        }
    }

    pub fn pitch_point(&self) -> Point2D {
        let penalty_top = (PITCH_WIDTH - PENALTY_AREA_WIDTH) / 2.0;
        let penalty_bottom = penalty_top + PENALTY_AREA_WIDTH;

        match self {
            PitchReferencePoint::TopLeftCorner => Point2D { x: 0.0, y: 0.0 },
            PitchReferencePoint::TopRightCorner => Point2D {
                x: PITCH_LENGTH,
                y: 0.0,
            },
            PitchReferencePoint::BottomRightCorner => Point2D {
                x: PITCH_LENGTH,
                y: PITCH_WIDTH,
            },
            PitchReferencePoint::BottomLeftCorner => Point2D {
                x: 0.0,
                y: PITCH_WIDTH,
            },
            PitchReferencePoint::CenterSpot => Point2D {
                x: PITCH_LENGTH / 2.0,
                y: PITCH_WIDTH / 2.0,
            },
            PitchReferencePoint::CenterLineTop => Point2D {
                x: PITCH_LENGTH / 2.0,
                y: 0.0,
            },
            PitchReferencePoint::CenterLineBottom => Point2D {
                x: PITCH_LENGTH / 2.0,
                y: PITCH_WIDTH,
            },
            PitchReferencePoint::CenterCircleTop => Point2D {
                x: PITCH_LENGTH / 2.0,
                y: PITCH_WIDTH / 2.0 - CENTER_CIRCLE_RADIUS,
            },
            PitchReferencePoint::CenterCircleBottom => Point2D {
                x: PITCH_LENGTH / 2.0,
                y: PITCH_WIDTH / 2.0 + CENTER_CIRCLE_RADIUS,
            },
            PitchReferencePoint::LeftPenaltySpot => Point2D {
                x: PENALTY_SPOT_DISTANCE,
                y: PITCH_WIDTH / 2.0,
            },
            PitchReferencePoint::RightPenaltySpot => Point2D {
                x: PITCH_LENGTH - PENALTY_SPOT_DISTANCE,
                y: PITCH_WIDTH / 2.0,
            },
            PitchReferencePoint::LeftPenaltyAreaTopRight => Point2D {
                x: PENALTY_AREA_DEPTH,
                y: penalty_top,
            },
            PitchReferencePoint::LeftPenaltyAreaBottomRight => Point2D {
                x: PENALTY_AREA_DEPTH,
                y: penalty_bottom,
            },
            PitchReferencePoint::RightPenaltyAreaTopLeft => Point2D {
                x: PITCH_LENGTH - PENALTY_AREA_DEPTH,
                y: penalty_top,
            },
            PitchReferencePoint::RightPenaltyAreaBottomLeft => Point2D {
                x: PITCH_LENGTH - PENALTY_AREA_DEPTH,
                y: penalty_bottom,
            },
        }
    }
}

impl std::fmt::Display for PitchReferencePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Coarse field-area awareness used throughout the UI and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchArea {
    DefensiveLeft,
    DefensiveCenter,
    DefensiveRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    AttackingLeft,
    AttackingCenter,
    AttackingRight,
}

impl std::fmt::Display for PitchArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PitchArea::DefensiveLeft => write!(f, "Defensive Left"),
            PitchArea::DefensiveCenter => write!(f, "Defensive Center"),
            PitchArea::DefensiveRight => write!(f, "Defensive Right"),
            PitchArea::MiddleLeft => write!(f, "Middle Left"),
            PitchArea::MiddleCenter => write!(f, "Middle Center"),
            PitchArea::MiddleRight => write!(f, "Middle Right"),
            PitchArea::AttackingLeft => write!(f, "Attacking Left"),
            PitchArea::AttackingCenter => write!(f, "Attacking Center"),
            PitchArea::AttackingRight => write!(f, "Attacking Right"),
        }
    }
}

/// Four image-to-pitch correspondences for manual or automatic homography calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomographyCalibration {
    /// Four clicked or auto-generated points in the image (pixel coords)
    pub image_points: [Option<Point2D>; 4],
    /// The real-world pitch landmarks those clicks correspond to.
    pub reference_points: [PitchReferencePoint; 4],
    /// When present, use these pitch points directly instead of named landmarks.
    pub explicit_pitch_points: Option<[Point2D; 4]>,
    pub mode: CalibrationMode,
}

impl HomographyCalibration {
    pub fn completion_count(&self) -> usize {
        self.image_points
            .iter()
            .filter(|point| point.is_some())
            .count()
    }

    pub fn is_ready(&self) -> bool {
        self.completion_count() == 4
    }

    pub fn correspondences(&self) -> Result<([Point2D; 4], [Point2D; 4])> {
        let mut image_points = [Point2D { x: 0.0, y: 0.0 }; 4];
        let mut pitch_points = [Point2D { x: 0.0, y: 0.0 }; 4];
        let mut seen = HashSet::new();

        for i in 0..4 {
            let reference_point = self.reference_points[i];
            if self.explicit_pitch_points.is_none() && !seen.insert(reference_point) {
                return Err(anyhow::anyhow!(
                    "Calibration points must use 4 different pitch landmarks"
                ));
            }

            let image_point = self.image_points[i].ok_or_else(|| {
                anyhow::anyhow!(
                    "Set calibration point #{} ({}) before running analysis",
                    i + 1,
                    reference_point.label()
                )
            })?;

            image_points[i] = image_point;
            pitch_points[i] = self
                .explicit_pitch_points
                .map(|points| points[i])
                .unwrap_or_else(|| reference_point.pitch_point());
        }

        Ok((image_points, pitch_points))
    }

    pub fn from_auto_detected(image_points: [Point2D; 4], pitch_points: [Point2D; 4]) -> Self {
        Self {
            image_points: image_points.map(Some),
            reference_points: [
                PitchReferencePoint::TopLeftCorner,
                PitchReferencePoint::TopRightCorner,
                PitchReferencePoint::BottomRightCorner,
                PitchReferencePoint::BottomLeftCorner,
            ],
            explicit_pitch_points: Some(pitch_points),
            mode: CalibrationMode::Automatic,
        }
    }
}

impl Default for HomographyCalibration {
    fn default() -> Self {
        Self {
            image_points: [None; 4],
            reference_points: [
                PitchReferencePoint::TopLeftCorner,
                PitchReferencePoint::TopRightCorner,
                PitchReferencePoint::BottomRightCorner,
                PitchReferencePoint::BottomLeftCorner,
            ],
            explicit_pitch_points: None,
            mode: CalibrationMode::Manual,
        }
    }
}

/// Homography transformer: maps image points to pitch coordinates
#[derive(Clone)]
pub struct PitchMapper {
    /// 3x3 homography matrix (image -> pitch)
    homography: Matrix3<f64>,
    /// Inverse homography (pitch -> image)
    inverse: Matrix3<f64>,
    pub calibration: HomographyCalibration,
}

impl PitchMapper {
    /// Compute homography from 4 point correspondences
    /// Uses DLT (Direct Linear Transform) algorithm
    pub fn from_calibration(calibration: HomographyCalibration) -> Result<Self> {
        let (image_points, pitch_points) = calibration.correspondences()?;
        let h = compute_homography(&image_points, &pitch_points)?;
        let inv = h
            .try_inverse()
            .ok_or_else(|| anyhow::anyhow!("Homography matrix is singular"))?;

        info!("Homography computed successfully ({})", calibration.mode);

        Ok(Self {
            homography: h,
            inverse: inv,
            calibration,
        })
    }

    /// Map a pixel coordinate to pitch coordinate (meters)
    pub fn image_to_pitch(&self, px: f64, py: f64) -> Point2D {
        let p = Vector3::new(px, py, 1.0);
        let result = self.homography * p;
        Point2D {
            x: result[0] / result[2],
            y: result[1] / result[2],
        }
    }

    /// Map a pitch coordinate (meters) to image pixel
    pub fn pitch_to_image(&self, mx: f64, my: f64) -> Point2D {
        let p = Vector3::new(mx, my, 1.0);
        let result = self.inverse * p;
        Point2D {
            x: result[0] / result[2],
            y: result[1] / result[2],
        }
    }

    /// Map bottom-center of a bounding box to pitch coords (feet position)
    pub fn bbox_to_pitch(&self, x1: f32, _y1: f32, x2: f32, y2: f32) -> Point2D {
        let foot_x = (x1 + x2) as f64 / 2.0;
        let foot_y = y2 as f64; // bottom of bbox = feet
        self.image_to_pitch(foot_x, foot_y)
    }
}

/// Compute 3x3 homography using DLT with 4 point correspondences
fn compute_homography(src: &[Point2D; 4], dst: &[Point2D; 4]) -> Result<Matrix3<f64>> {
    let mut a = nalgebra::DMatrix::<f64>::zeros(8, 9);

    for i in 0..4 {
        let (sx, sy) = (src[i].x, src[i].y);
        let (dx, dy) = (dst[i].x, dst[i].y);

        let row1 = i * 2;
        let row2 = i * 2 + 1;

        a[(row1, 0)] = -sx;
        a[(row1, 1)] = -sy;
        a[(row1, 2)] = -1.0;
        a[(row1, 6)] = dx * sx;
        a[(row1, 7)] = dx * sy;
        a[(row1, 8)] = dx;

        a[(row2, 3)] = -sx;
        a[(row2, 4)] = -sy;
        a[(row2, 5)] = -1.0;
        a[(row2, 6)] = dy * sx;
        a[(row2, 7)] = dy * sy;
        a[(row2, 8)] = dy;
    }

    let svd = a.svd(true, true);
    let v_t = svd.v_t.ok_or_else(|| anyhow::anyhow!("SVD failed"))?;
    let h_vec = v_t.row(8);

    let h = Matrix3::new(
        h_vec[0], h_vec[1], h_vec[2], h_vec[3], h_vec[4], h_vec[5], h_vec[6], h_vec[7], h_vec[8],
    );

    let scale = h[(2, 2)];
    if scale.abs() < 1e-10 {
        return Err(anyhow::anyhow!("Degenerate homography"));
    }

    Ok(h / scale)
}

/// Compute Euclidean distance between two pitch points (meters)
pub fn pitch_distance(a: &Point2D, b: &Point2D) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Classify a pitch position into a coarse 3x3 tactical area.
pub fn classify_pitch_area(pos: &Point2D) -> PitchArea {
    let x = pos.x.clamp(0.0, PITCH_LENGTH - f64::EPSILON);
    let y = pos.y.clamp(0.0, PITCH_WIDTH - f64::EPSILON);

    let third_x = if x < PITCH_LENGTH / 3.0 {
        0
    } else if x < PITCH_LENGTH * 2.0 / 3.0 {
        1
    } else {
        2
    };

    let lane_y = if y < PITCH_WIDTH / 3.0 {
        0
    } else if y < PITCH_WIDTH * 2.0 / 3.0 {
        1
    } else {
        2
    };

    match (third_x, lane_y) {
        (0, 0) => PitchArea::DefensiveLeft,
        (0, 1) => PitchArea::DefensiveCenter,
        (0, 2) => PitchArea::DefensiveRight,
        (1, 0) => PitchArea::MiddleLeft,
        (1, 1) => PitchArea::MiddleCenter,
        (1, 2) => PitchArea::MiddleRight,
        (2, 0) => PitchArea::AttackingLeft,
        (2, 1) => PitchArea::AttackingCenter,
        (2, 2) => PitchArea::AttackingRight,
        _ => PitchArea::MiddleCenter,
    }
}
