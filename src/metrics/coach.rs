use super::TimedPosition;
use crate::detection::COCO_SPORTS_BALL;
use crate::pitch_mapping::PitchMapper;
use crate::pitch_mapping::{pitch_distance, PitchArea, Point2D, PITCH_LENGTH};
use crate::tracker::TrackingResult;
use crate::video_processor::VideoFrame;
use image::RgbImage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TeamId {
    TeamA,
    TeamB,
}

impl std::fmt::Display for TeamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeamId::TeamA => write!(f, "Team A"),
            TeamId::TeamB => write!(f, "Team B"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PhaseLabel {
    BuildUp,
    Progression,
    FinalThird,
    DefensiveBlock,
    HighPress,
    AttackingTransition,
    DefensiveTransition,
}

impl std::fmt::Display for PhaseLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhaseLabel::BuildUp => write!(f, "Build-up"),
            PhaseLabel::Progression => write!(f, "Progression"),
            PhaseLabel::FinalThird => write!(f, "Final Third"),
            PhaseLabel::DefensiveBlock => write!(f, "Defensive Block"),
            PhaseLabel::HighPress => write!(f, "High Press"),
            PhaseLabel::AttackingTransition => write!(f, "Attacking Transition"),
            PhaseLabel::DefensiveTransition => write!(f, "Defensive Transition"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamShapeMetrics {
    pub team: TeamId,
    pub width_m: f64,
    pub depth_m: f64,
    pub compactness_m2: f64,
    pub line_height_m: f64,
    pub centroid: Point2D,
    pub players_counted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameTeamShape {
    pub timestamp_secs: f64,
    pub team: TeamId,
    pub width_m: f64,
    pub depth_m: f64,
    pub compactness_m2: f64,
    pub line_height_m: f64,
    pub support_score: f64,
    pub rest_defense_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamLineMetrics {
    pub team: TeamId,
    pub back_line_height_m: f64,
    pub midfield_height_m: f64,
    pub front_line_height_m: f64,
    pub back_to_mid_spacing_m: f64,
    pub mid_to_front_spacing_m: f64,
    pub midfield_support_count: usize,
    pub between_lines_occupation_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLineSample {
    pub timestamp_secs: f64,
    pub team: TeamId,
    pub back_to_mid_spacing_m: f64,
    pub mid_to_front_spacing_m: f64,
    pub between_lines_occupation_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PossessionContext {
    pub timestamp_secs: f64,
    pub team_in_possession: TeamId,
    pub ball_position: Point2D,
    pub support_near_ball: usize,
    pub central_receiver_available: bool,
    pub spare_player_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseEvent {
    pub timestamp_secs: f64,
    pub label: PhaseLabel,
    pub area: PitchArea,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralAlert {
    pub team: TeamId,
    pub title: String,
    pub description: String,
    pub severity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorSignature {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub saturation: f32,
}

/// Convert a jersey color signature to a short human-readable name
/// ("Red", "Blue", "White", "Dark", "Yellow", etc.).
/// r/g/b are expected in 0..1.
pub fn color_signature_name(c: &ColorSignature) -> &'static str {
    let r = c.r.clamp(0.0, 1.0);
    let g = c.g.clamp(0.0, 1.0);
    let b = c.b.clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) * 0.5;
    let sat = c.saturation.clamp(0.0, 1.0);

    // Low saturation → grayscale: White / Gray / Black / Dark
    if sat < 0.18 {
        if lightness > 0.78 {
            return "White";
        } else if lightness > 0.45 {
            return "Gray";
        } else if lightness > 0.22 {
            return "Dark";
        } else {
            return "Black";
        }
    }

    // Hue buckets using dominant channel comparisons
    // Red: r dominant, g & b similarly lower
    // Yellow: r & g high, b low
    // Green: g dominant
    // Cyan: g & b high, r low
    // Blue: b dominant
    // Magenta/Pink: r & b high, g low; split by lightness
    let r_dom = r >= g && r >= b;
    let g_dom = g >= r && g >= b;
    let b_dom = b >= r && b >= g;

    if r_dom && g > 0.55 && b < 0.45 {
        return if lightness > 0.55 { "Yellow" } else { "Orange" };
    }
    if r_dom && b > 0.45 && g < 0.45 {
        return if lightness > 0.65 { "Pink" } else { "Magenta" };
    }
    if g_dom && b > 0.55 && r < 0.45 {
        return "Cyan";
    }
    if r_dom {
        return if lightness < 0.35 { "Maroon" } else { "Red" };
    }
    if g_dom {
        return if lightness < 0.35 { "Dark Green" } else { "Green" };
    }
    if b_dom {
        return if lightness < 0.35 {
            "Navy"
        } else if lightness > 0.70 {
            "Light Blue"
        } else {
            "Blue"
        };
    }
    "Team"
}

/// Return a short display label for a team based on its jersey color,
/// falling back to "Team A"/"Team B" if color data is unavailable or
/// both teams resolve to the same name (in which case we disambiguate
/// with Light/Dark prefix or fall back to the generic label).
pub fn team_label(team: TeamId, metrics: &CoachMetrics) -> String {
    let own = metrics.team_colors.get(&team);
    let other_team = match team {
        TeamId::TeamA => TeamId::TeamB,
        TeamId::TeamB => TeamId::TeamA,
    };
    let other = metrics.team_colors.get(&other_team);

    let Some(own_sig) = own else {
        return team.to_string();
    };
    let own_name = color_signature_name(own_sig);

    if let Some(other_sig) = other {
        let other_name = color_signature_name(other_sig);
        if own_name == other_name {
            // Disambiguate by lightness
            let own_light = own_sig.r + own_sig.g + own_sig.b;
            let other_light = other_sig.r + other_sig.g + other_sig.b;
            if (own_light - other_light).abs() < 0.15 {
                // Truly indistinguishable — fall back to generic
                return team.to_string();
            }
            let prefix = if own_light >= other_light {
                "Light"
            } else {
                "Dark"
            };
            return format!("{} {}", prefix, own_name);
        }
    }
    own_name.to_string()
}

/// Return the sampled jersey color of a team as sRGB bytes (0..=255) if
/// a signature was captured. Brightens very dark colors and desaturates
/// extremely saturated ones slightly so dots remain visible against the
/// dark UI background.
pub fn team_display_rgb(team: TeamId, metrics: &CoachMetrics) -> Option<(u8, u8, u8)> {
    let sig = metrics.team_colors.get(&team)?;
    let mut r = sig.r.clamp(0.0, 1.0);
    let mut g = sig.g.clamp(0.0, 1.0);
    let mut b = sig.b.clamp(0.0, 1.0);
    // Pull very dark colors up so they register against dark chrome.
    let light = (r + g + b) / 3.0;
    if light < 0.35 {
        let boost = (0.35 - light) * 1.4;
        r = (r + boost).min(1.0);
        g = (g + boost).min(1.0);
        b = (b + boost).min(1.0);
    }
    Some(((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoachMetrics {
    pub team_assignments: HashMap<u32, TeamId>,
    pub team_shapes: HashMap<TeamId, TeamShapeMetrics>,
    pub frame_team_shapes: Vec<FrameTeamShape>,
    pub team_lines: HashMap<TeamId, TeamLineMetrics>,
    pub frame_line_samples: Vec<FrameLineSample>,
    pub team_colors: HashMap<TeamId, ColorSignature>,
    pub dominant_phase: Option<PhaseLabel>,
    pub phase_events: Vec<PhaseEvent>,
    pub possession_contexts: Vec<PossessionContext>,
    pub structural_alerts: Vec<StructuralAlert>,
}

pub fn compute_coach_metrics(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    frames: &[VideoFrame],
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    dominant_areas: &HashMap<u32, PitchArea>,
) -> CoachMetrics {
    let color_signatures = collect_track_color_signatures(tracking, frames);
    let team_assignments = assign_teams(player_positions, &color_signatures);
    let team_shapes = compute_team_shapes(player_positions, &team_assignments);
    let frame_team_shapes =
        compute_frame_team_shapes(tracking, player_positions, &team_assignments);
    let team_lines = compute_team_lines(player_positions, &team_assignments);
    let frame_line_samples =
        compute_frame_line_samples(tracking, player_positions, &team_assignments);
    let phase_events = detect_phase_events(player_positions, &team_assignments, dominant_areas);
    let dominant_phase = most_common_phase(&phase_events);
    let possession_contexts =
        compute_possession_contexts(tracking, mapper, player_positions, &team_assignments);
    let structural_alerts = compute_structural_alerts(
        &team_shapes,
        &frame_team_shapes,
        &team_lines,
        &frame_line_samples,
        &possession_contexts,
        dominant_areas,
        &team_assignments,
    );
    let team_colors = summarize_team_colors(&team_assignments, &color_signatures);

    CoachMetrics {
        team_assignments,
        team_shapes,
        frame_team_shapes,
        team_lines,
        frame_line_samples,
        team_colors,
        dominant_phase,
        phase_events,
        possession_contexts,
        structural_alerts,
    }
}

fn collect_track_color_signatures(
    tracking: &TrackingResult,
    frames: &[VideoFrame],
) -> HashMap<u32, Vec<ColorSignature>> {
    let frame_lookup: HashMap<u64, &VideoFrame> =
        frames.iter().map(|frame| (frame.index, frame)).collect();
    let mut signatures: HashMap<u32, Vec<ColorSignature>> = HashMap::new();

    for frame_tracks in tracking
        .frame_tracks
        .iter()
        .step_by((tracking.frame_tracks.len() / 20).max(1))
    {
        let Some(frame) = frame_lookup.get(&frame_tracks.frame_index) else {
            continue;
        };

        for track in &frame_tracks.tracks {
            if track.class_name != "Player" {
                continue;
            }
            if let Some(signature) = extract_jersey_signature(
                &frame.image,
                track.bbox.x1,
                track.bbox.y1,
                track.bbox.x2,
                track.bbox.y2,
            ) {
                signatures
                    .entry(track.track_id)
                    .or_default()
                    .push(signature);
            }
        }
    }

    signatures
}

fn assign_teams(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    color_signatures: &HashMap<u32, Vec<ColorSignature>>,
) -> HashMap<u32, TeamId> {
    let mut track_features: Vec<(u32, f32, f32, f64)> = player_positions
        .iter()
        .filter_map(|(&track_id, positions)| {
            let avg_x = positions.iter().map(|p| p.pitch_pos.x).sum::<f64>()
                / positions.len().max(1) as f64;
            let color = average_color_signature(color_signatures.get(&track_id)?);
            Some((track_id, color.r - color.b, color.saturation, avg_x))
        })
        .collect();

    if track_features.len() < 2 {
        return HashMap::new();
    }

    track_features.sort_by(|a, b| {
        let lhs = a.1 + a.2 * 0.45;
        let rhs = b.1 + b.2 * 0.45;
        lhs.total_cmp(&rhs)
    });

    let split = track_features.len() / 2;
    let mut assignments = HashMap::new();
    for (idx, (track_id, _, _, avg_x)) in track_features.into_iter().enumerate() {
        let mut team = if idx < split {
            TeamId::TeamA
        } else {
            TeamId::TeamB
        };
        if avg_x > PITCH_LENGTH * 0.82 && idx < split / 2 {
            team = TeamId::TeamB;
        }
        assignments.insert(track_id, team);
    }
    assignments
}

fn compute_team_shapes(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
) -> HashMap<TeamId, TeamShapeMetrics> {
    let mut grouped: HashMap<TeamId, Vec<Point2D>> = HashMap::new();
    for (&track_id, positions) in player_positions {
        if let Some(team) = team_assignments.get(&track_id) {
            let sample_points = positions.iter().step_by((positions.len() / 8).max(1));
            grouped
                .entry(*team)
                .or_default()
                .extend(sample_points.map(|p| p.pitch_pos));
        }
    }

    grouped
        .into_iter()
        .map(|(team, points)| (team, shape_from_points(team, &points)))
        .collect()
}

fn compute_frame_team_shapes(
    tracking: &TrackingResult,
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
) -> Vec<FrameTeamShape> {
    let position_lookup: HashMap<(u32, u64), Point2D> = player_positions
        .iter()
        .flat_map(|(&track_id, positions)| {
            positions
                .iter()
                .map(move |p| ((track_id, p.frame_index), p.pitch_pos))
        })
        .collect();

    let mut shapes = Vec::new();
    for frame_tracks in tracking
        .frame_tracks
        .iter()
        .step_by((tracking.frame_tracks.len() / 18).max(1))
    {
        let mut by_team: HashMap<TeamId, Vec<Point2D>> = HashMap::new();
        for track in &frame_tracks.tracks {
            if let Some(team) = team_assignments.get(&track.track_id) {
                if let Some(pos) = position_lookup.get(&(track.track_id, frame_tracks.frame_index))
                {
                    by_team.entry(*team).or_default().push(*pos);
                }
            }
        }

        for (team, points) in by_team {
            if points.len() < 4 {
                continue;
            }
            let shape = shape_from_points(team, &points);
            let support_score = compute_support_score(&points);
            let rest_defense_score = compute_rest_defense_score(&points);
            shapes.push(FrameTeamShape {
                timestamp_secs: frame_tracks.timestamp_secs,
                team,
                width_m: shape.width_m,
                depth_m: shape.depth_m,
                compactness_m2: shape.compactness_m2,
                line_height_m: shape.line_height_m,
                support_score,
                rest_defense_score,
            });
        }
    }
    shapes
}

fn compute_team_lines(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
) -> HashMap<TeamId, TeamLineMetrics> {
    let mut by_team: HashMap<TeamId, Vec<Point2D>> = HashMap::new();
    for (&track_id, positions) in player_positions {
        if let Some(team) = team_assignments.get(&track_id) {
            let sample_points = positions.iter().step_by((positions.len() / 8).max(1));
            by_team
                .entry(*team)
                .or_default()
                .extend(sample_points.map(|p| p.pitch_pos));
        }
    }

    by_team
        .into_iter()
        .filter_map(|(team, mut points)| compute_line_metrics(team, &mut points))
        .collect()
}

fn compute_frame_line_samples(
    tracking: &TrackingResult,
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
) -> Vec<FrameLineSample> {
    let position_lookup: HashMap<(u32, u64), Point2D> = player_positions
        .iter()
        .flat_map(|(&track_id, positions)| {
            positions
                .iter()
                .map(move |p| ((track_id, p.frame_index), p.pitch_pos))
        })
        .collect();

    let mut samples = Vec::new();
    for frame_tracks in tracking
        .frame_tracks
        .iter()
        .step_by((tracking.frame_tracks.len() / 18).max(1))
    {
        let mut by_team: HashMap<TeamId, Vec<Point2D>> = HashMap::new();
        for track in &frame_tracks.tracks {
            if let Some(team) = team_assignments.get(&track.track_id) {
                if let Some(pos) = position_lookup.get(&(track.track_id, frame_tracks.frame_index))
                {
                    by_team.entry(*team).or_default().push(*pos);
                }
            }
        }

        for (team, mut points) in by_team {
            if let Some((_, metrics)) = compute_line_metrics(team, &mut points) {
                samples.push(FrameLineSample {
                    timestamp_secs: frame_tracks.timestamp_secs,
                    team,
                    back_to_mid_spacing_m: metrics.back_to_mid_spacing_m,
                    mid_to_front_spacing_m: metrics.mid_to_front_spacing_m,
                    between_lines_occupation_score: metrics.between_lines_occupation_score,
                });
            }
        }
    }

    samples
}

fn compute_possession_contexts(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
) -> Vec<PossessionContext> {
    let position_lookup: HashMap<(u32, u64), Point2D> = player_positions
        .iter()
        .flat_map(|(&track_id, positions)| {
            positions
                .iter()
                .map(move |p| ((track_id, p.frame_index), p.pitch_pos))
        })
        .collect();

    let mut contexts = Vec::new();
    for frame_tracks in tracking
        .frame_tracks
        .iter()
        .step_by((tracking.frame_tracks.len() / 18).max(1))
    {
        let ball = frame_tracks
            .tracks
            .iter()
            .find(|track| track.class_id == COCO_SPORTS_BALL);
        let Some(ball) = ball else {
            continue;
        };

        let ball_pos = mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);
        if !ball_pos.x.is_finite() || !ball_pos.y.is_finite() {
            continue;
        }

        let mut nearby: Vec<(u32, TeamId, Point2D, f64)> = frame_tracks
            .tracks
            .iter()
            .filter(|track| track.class_name == "Player")
            .filter_map(|track| {
                let team = *team_assignments.get(&track.track_id)?;
                let pos = *position_lookup.get(&(track.track_id, frame_tracks.frame_index))?;
                if !pos.x.is_finite() || !pos.y.is_finite() {
                    return None;
                }
                let dist = pitch_distance(&ball_pos, &pos);
                if !dist.is_finite() {
                    return None;
                }
                Some((track.track_id, team, pos, dist))
            })
            .collect();

        nearby.sort_by(|a, b| a.3.total_cmp(&b.3));
        let Some((carrier_track_id, team_in_possession, carrier_pos, _)) = nearby.first().copied()
        else {
            continue;
        };

        let support_near_ball = nearby
            .iter()
            .filter(|(track_id, team, _, dist)| {
                *track_id != carrier_track_id && *team == team_in_possession && *dist <= 14.0
            })
            .count();
        let central_receiver_available = nearby.iter().any(|(_, team, pos, dist)| {
            *team == team_in_possession
                && *dist > 5.0
                && *dist <= 22.0
                && (pos.y - carrier_pos.y).abs() <= 12.0
                && (pos.x - carrier_pos.x).abs() >= 4.0
        });

        let own_support = nearby
            .iter()
            .filter(|(track_id, team, _, dist)| {
                *track_id != carrier_track_id && *team == team_in_possession && *dist <= 12.0
            })
            .count() as f64;
        let opp_pressure = nearby
            .iter()
            .filter(|(_, team, _, dist)| *team != team_in_possession && *dist <= 12.0)
            .count() as f64;
        let spare_player_score = ((own_support - opp_pressure + 2.0) / 4.0).clamp(0.0, 1.0);

        contexts.push(PossessionContext {
            timestamp_secs: frame_tracks.timestamp_secs,
            team_in_possession,
            ball_position: ball_pos,
            support_near_ball,
            central_receiver_available,
            spare_player_score,
        });
    }

    contexts
}

fn detect_phase_events(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
    dominant_areas: &HashMap<u32, PitchArea>,
) -> Vec<PhaseEvent> {
    let mut events = Vec::new();

    for (&track_id, positions) in player_positions {
        let Some(team) = team_assignments.get(&track_id) else {
            continue;
        };
        let Some(area) = dominant_areas.get(&track_id) else {
            continue;
        };

        let avg_x =
            positions.iter().map(|p| p.pitch_pos.x).sum::<f64>() / positions.len().max(1) as f64;
        let avg_speed_proxy = if positions.len() > 1 {
            let first = positions.first().unwrap().pitch_pos;
            let last = positions.last().unwrap().pitch_pos;
            ((last.x - first.x).abs() + (last.y - first.y).abs()) / positions.len() as f64
        } else {
            0.0
        };

        let label = if avg_speed_proxy > 1.4 {
            if avg_x > PITCH_LENGTH / 2.0 {
                PhaseLabel::AttackingTransition
            } else {
                PhaseLabel::DefensiveTransition
            }
        } else if avg_x < PITCH_LENGTH * 0.25 {
            PhaseLabel::BuildUp
        } else if avg_x < PITCH_LENGTH * 0.6 {
            PhaseLabel::Progression
        } else if avg_x < PITCH_LENGTH * 0.8 {
            PhaseLabel::FinalThird
        } else if *team == TeamId::TeamA {
            PhaseLabel::HighPress
        } else {
            PhaseLabel::DefensiveBlock
        };

        events.push(PhaseEvent {
            timestamp_secs: positions.first().map(|p| p.timestamp_secs).unwrap_or(0.0),
            label,
            area: *area,
        });
    }

    events.sort_by(|a, b| a.timestamp_secs.total_cmp(&b.timestamp_secs));
    events
}

fn compute_structural_alerts(
    team_shapes: &HashMap<TeamId, TeamShapeMetrics>,
    frame_team_shapes: &[FrameTeamShape],
    team_lines: &HashMap<TeamId, TeamLineMetrics>,
    frame_line_samples: &[FrameLineSample],
    possession_contexts: &[PossessionContext],
    dominant_areas: &HashMap<u32, PitchArea>,
    team_assignments: &HashMap<u32, TeamId>,
) -> Vec<StructuralAlert> {
    let mut alerts = Vec::new();

    for (team, shape) in team_shapes {
        if shape.width_m < 20.0 {
            alerts.push(StructuralAlert {
                team: *team,
                title: "Team too narrow".to_string(),
                description: format!(
                    "{} are only {:.1}m wide on average. Harder to stretch the opponent or protect the far side.",
                    team, shape.width_m
                ),
                severity: 0.82,
            });
        }

        if shape.depth_m > 42.0 {
            alerts.push(StructuralAlert {
                team: *team,
                title: "Lines look stretched".to_string(),
                description: format!(
                    "{} show {:.1}m of depth. Space may be opening between units.",
                    team, shape.depth_m
                ),
                severity: 0.76,
            });
        }

        if shape.line_height_m < 18.0 {
            alerts.push(StructuralAlert {
                team: *team,
                title: "Back line too deep".to_string(),
                description: format!(
                    "{} average line height is only {:.1}m. Team may be sinking too early and inviting pressure.",
                    team, shape.line_height_m
                ),
                severity: 0.72,
            });
        }

        if let Some(lines) = team_lines.get(team) {
            if lines.back_to_mid_spacing_m > 16.0 {
                alerts.push(StructuralAlert {
                    team: *team,
                    title: "Back line disconnected from midfield".to_string(),
                    description: format!(
                        "{} show {:.1}m between back and midfield lines. That gap is large enough for opponents to play through.",
                        team, lines.back_to_mid_spacing_m
                    ),
                    severity: 0.87,
                });
            }

            if lines.mid_to_front_spacing_m > 18.0 {
                alerts.push(StructuralAlert {
                    team: *team,
                    title: "Front line too far from midfield".to_string(),
                    description: format!(
                        "{} have {:.1}m between midfield and front line. Pressing and combination play may break apart.",
                        team, lines.mid_to_front_spacing_m
                    ),
                    severity: 0.73,
                });
            }

            if lines.between_lines_occupation_score < 0.35 {
                alerts.push(StructuralAlert {
                    team: *team,
                    title: "Between-lines occupation is weak".to_string(),
                    description: format!(
                        "{} are not keeping enough players between the opponent's lines. Harder to connect progression into attack.",
                        team
                    ),
                    severity: 0.82,
                });
            }
        }
    }

    for team in [TeamId::TeamA, TeamId::TeamB] {
        let samples: Vec<_> = frame_team_shapes
            .iter()
            .filter(|shape| shape.team == team)
            .collect();
        if !samples.is_empty() {
            let avg_compactness = samples
                .iter()
                .map(|shape| shape.compactness_m2)
                .sum::<f64>()
                / samples.len() as f64;
            let avg_line_height =
                samples.iter().map(|shape| shape.line_height_m).sum::<f64>() / samples.len() as f64;
            let avg_support =
                samples.iter().map(|shape| shape.support_score).sum::<f64>() / samples.len() as f64;
            let avg_rest_defense = samples
                .iter()
                .map(|shape| shape.rest_defense_score)
                .sum::<f64>()
                / samples.len() as f64;

            if avg_compactness > 1400.0 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Poor compactness".to_string(),
                    description: format!(
                        "{} average compactness is {:.0} m^2. Team shape looks loose and distances may be too big.",
                        team, avg_compactness
                    ),
                    severity: 0.79,
                });
            }

            if avg_line_height > PITCH_LENGTH * 0.68 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Very high line height".to_string(),
                    description: format!(
                        "{} are holding a very high average line ({:.1}m). Good for pressure, risky if rest defense is poor.",
                        team, avg_line_height
                    ),
                    severity: 0.64,
                });
            }

            if avg_support < 0.42 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Midfield looks disconnected".to_string(),
                    description: format!(
                        "{} show weak short-distance support patterns. Central links may be missing around the ball.",
                        team
                    ),
                    severity: 0.81,
                });
            }

            if avg_rest_defense < 0.38 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Rest defense weak".to_string(),
                    description: format!(
                        "{} do not keep enough cover behind the attack in sampled frames. Transition protection may be poor.",
                        team
                    ),
                    severity: 0.86,
                });
            }
        }

        let line_samples: Vec<_> = frame_line_samples
            .iter()
            .filter(|sample| sample.team == team)
            .collect();
        if !line_samples.is_empty() {
            let avg_between_lines = line_samples
                .iter()
                .map(|sample| sample.between_lines_occupation_score)
                .sum::<f64>()
                / line_samples.len() as f64;
            if avg_between_lines < 0.40 {
                alerts.push(StructuralAlert {
                    team,
                    title: "No stable option between lines".to_string(),
                    description: format!(
                        "{} rarely have a player living between the lines in sampled frames. The team may be circulating around the block instead of through it.",
                        team
                    ),
                    severity: 0.77,
                });
            }
        }

        let contexts: Vec<_> = possession_contexts
            .iter()
            .filter(|ctx| ctx.team_in_possession == team)
            .collect();
        if !contexts.is_empty() {
            let avg_support_near_ball = contexts
                .iter()
                .map(|ctx| ctx.support_near_ball as f64)
                .sum::<f64>()
                / contexts.len() as f64;
            let central_receiver_rate = contexts
                .iter()
                .filter(|ctx| ctx.central_receiver_available)
                .count() as f64
                / contexts.len() as f64;
            let avg_spare = contexts
                .iter()
                .map(|ctx| ctx.spare_player_score)
                .sum::<f64>()
                / contexts.len() as f64;

            if avg_support_near_ball < 2.0 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Ball carrier lacks support".to_string(),
                    description: format!(
                        "{} average only {:.1} teammates near the ball. The carrier may be isolated too often.",
                        team, avg_support_near_ball
                    ),
                    severity: 0.78,
                });
            }

            if central_receiver_rate < 0.35 {
                alerts.push(StructuralAlert {
                    team,
                    title: "No central receiver available".to_string(),
                    description: format!(
                        "{} rarely show a central receiving option near the ball. Progression may be forced wide or backwards.",
                        team
                    ),
                    severity: 0.83,
                });
            }

            if avg_spare > 0.65 {
                alerts.push(StructuralAlert {
                    team,
                    title: "Spare player around the ball".to_string(),
                    description: format!(
                        "{} often appear to have a spare player around the ball. Good sign for pressure resistance or overload creation.",
                        team
                    ),
                    severity: 0.57,
                });
            }
        }
    }

    let mut team_area_counts: HashMap<(TeamId, PitchArea), usize> = HashMap::new();
    for (&track_id, area) in dominant_areas {
        if let Some(team) = team_assignments.get(&track_id) {
            *team_area_counts.entry((*team, *area)).or_insert(0) += 1;
        }
    }

    for team in [TeamId::TeamA, TeamId::TeamB] {
        let left = area_band_total(
            &team_area_counts,
            team,
            &[
                PitchArea::AttackingLeft,
                PitchArea::MiddleLeft,
                PitchArea::DefensiveLeft,
            ],
        );
        let center = area_band_total(
            &team_area_counts,
            team,
            &[
                PitchArea::AttackingCenter,
                PitchArea::MiddleCenter,
                PitchArea::DefensiveCenter,
            ],
        );
        let right = area_band_total(
            &team_area_counts,
            team,
            &[
                PitchArea::AttackingRight,
                PitchArea::MiddleRight,
                PitchArea::DefensiveRight,
            ],
        );

        if left >= right.saturating_mul(2).max(4) {
            alerts.push(StructuralAlert {
                team,
                title: "Left-side bias".to_string(),
                description: format!(
                    "{} are heavily tilted to the left side. Opponent can anticipate circulation there.",
                    team
                ),
                severity: 0.61,
            });
        } else if right >= left.saturating_mul(2).max(4) {
            alerts.push(StructuralAlert {
                team,
                title: "Right-side bias".to_string(),
                description: format!(
                    "{} are heavily tilted to the right side. Useful if intentional, predictable if repeated.",
                    team
                ),
                severity: 0.61,
            });
        }

        if center <= left.min(right) / 2 && center <= 2 {
            alerts.push(StructuralAlert {
                team,
                title: "Central occupation is weak".to_string(),
                description: format!(
                    "{} are not occupying the central lane enough. The middle of the pitch may be available to the opponent.",
                    team
                ),
                severity: 0.84,
            });
        }

        if left >= center + 3 || right >= center + 3 {
            alerts.push(StructuralAlert {
                team,
                title: "Wide overload detected".to_string(),
                description: format!(
                    "{} are creating a visible wide overload. Useful if it is deliberate and supported, risky if the far side is empty.",
                    team
                ),
                severity: 0.58,
            });
        }

        if center >= left + 3 && center >= right + 3 {
            alerts.push(StructuralAlert {
                team,
                title: "Spare player available centrally".to_string(),
                description: format!(
                    "{} appear to keep more occupation in the central lane than on either wing. There may be a spare player between lines.",
                    team
                ),
                severity: 0.63,
            });
        }
    }

    alerts
}

fn summarize_team_colors(
    team_assignments: &HashMap<u32, TeamId>,
    color_signatures: &HashMap<u32, Vec<ColorSignature>>,
) -> HashMap<TeamId, ColorSignature> {
    let mut grouped: HashMap<TeamId, Vec<ColorSignature>> = HashMap::new();
    for (&track_id, team) in team_assignments {
        if let Some(colors) = color_signatures.get(&track_id) {
            grouped
                .entry(*team)
                .or_default()
                .extend(colors.iter().cloned());
        }
    }

    grouped
        .into_iter()
        .map(|(team, colors)| (team, average_color_signature(&colors)))
        .collect()
}

fn compute_line_metrics(team: TeamId, points: &mut [Point2D]) -> Option<(TeamId, TeamLineMetrics)> {
    let mut points: Vec<Point2D> = points
        .iter()
        .copied()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .collect();

    if points.len() < 6 {
        return None;
    }

    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    let third = (points.len() / 3).max(1);
    let back_line = &points[..third.min(points.len())];
    let midfield = &points[third.min(points.len())..(third * 2).min(points.len())];
    let front_line = &points[(third * 2).min(points.len())..];
    if midfield.is_empty() || front_line.is_empty() {
        return None;
    }

    let back_height = avg_x(back_line);
    let midfield_height = avg_x(midfield);
    let front_height = avg_x(front_line);
    let between_low = (back_height + midfield_height) / 2.0;
    let between_high = (midfield_height + front_height) / 2.0;
    let between_lines_count = points
        .iter()
        .filter(|p| p.x > between_low && p.x < between_high)
        .count();

    Some((
        team,
        TeamLineMetrics {
            team,
            back_line_height_m: back_height,
            midfield_height_m: midfield_height,
            front_line_height_m: front_height,
            back_to_mid_spacing_m: (midfield_height - back_height).max(0.0),
            mid_to_front_spacing_m: (front_height - midfield_height).max(0.0),
            midfield_support_count: midfield.len(),
            between_lines_occupation_score: between_lines_count as f64 / points.len() as f64,
        },
    ))
}

fn shape_from_points(team: TeamId, points: &[Point2D]) -> TeamShapeMetrics {
    let min_x = points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    let min_y = points.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::MIN, f64::max);
    let centroid = Point2D {
        x: points.iter().map(|p| p.x).sum::<f64>() / points.len().max(1) as f64,
        y: points.iter().map(|p| p.y).sum::<f64>() / points.len().max(1) as f64,
    };
    TeamShapeMetrics {
        team,
        width_m: (max_y - min_y).max(0.0),
        depth_m: (max_x - min_x).max(0.0),
        compactness_m2: ((max_x - min_x) * (max_y - min_y)).max(0.0),
        line_height_m: centroid.x,
        centroid,
        players_counted: points.len(),
    }
}

fn extract_jersey_signature(
    image: &RgbImage,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
) -> Option<ColorSignature> {
    let width = image.width() as i32;
    let height = image.height() as i32;
    let left = x1.floor() as i32;
    let right = x2.ceil() as i32;
    let top = y1.floor() as i32;
    let bottom = y2.ceil() as i32;
    if right <= left || bottom <= top {
        return None;
    }

    let jersey_top = top + ((bottom - top) as f32 * 0.18) as i32;
    let jersey_bottom = top + ((bottom - top) as f32 * 0.52) as i32;
    let jersey_left = left + ((right - left) as f32 * 0.18) as i32;
    let jersey_right = right - ((right - left) as f32 * 0.18) as i32;

    let mut sum_r = 0.0f32;
    let mut sum_g = 0.0f32;
    let mut sum_b = 0.0f32;
    let mut sum_sat = 0.0f32;
    let mut count = 0.0f32;

    for y in jersey_top.max(0)..jersey_bottom.min(height) {
        for x in jersey_left.max(0)..jersey_right.min(width) {
            let pixel = image.get_pixel(x as u32, y as u32).0;
            let r = pixel[0] as f32 / 255.0;
            let g = pixel[1] as f32 / 255.0;
            let b = pixel[2] as f32 / 255.0;
            let max = r.max(g.max(b));
            let min = r.min(g.min(b));
            let sat = if max > 0.0 { (max - min) / max } else { 0.0 };

            if g > r * 1.1 && g > b * 1.1 {
                continue;
            }

            sum_r += r;
            sum_g += g;
            sum_b += b;
            sum_sat += sat;
            count += 1.0;
        }
    }

    (count > 12.0).then_some(ColorSignature {
        r: sum_r / count,
        g: sum_g / count,
        b: sum_b / count,
        saturation: sum_sat / count,
    })
}

fn average_color_signature(signatures: &[ColorSignature]) -> ColorSignature {
    let count = signatures.len().max(1) as f32;
    ColorSignature {
        r: signatures.iter().map(|c| c.r).sum::<f32>() / count,
        g: signatures.iter().map(|c| c.g).sum::<f32>() / count,
        b: signatures.iter().map(|c| c.b).sum::<f32>() / count,
        saturation: signatures.iter().map(|c| c.saturation).sum::<f32>() / count,
    }
}

fn compute_support_score(points: &[Point2D]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut close_links = 0usize;
    let mut total_pairs = 0usize;
    for i in 0..points.len() {
        for j in (i + 1)..points.len() {
            total_pairs += 1;
            let dx = points[i].x - points[j].x;
            let dy = points[i].y - points[j].y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 18.0 {
                close_links += 1;
            }
        }
    }
    close_links as f64 / total_pairs.max(1) as f64
}

fn compute_rest_defense_score(points: &[Point2D]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let behind_ball_line = points.iter().filter(|p| p.x < PITCH_LENGTH * 0.45).count();
    (behind_ball_line as f64 / points.len() as f64).clamp(0.0, 1.0)
}

fn most_common_phase(events: &[PhaseEvent]) -> Option<PhaseLabel> {
    events
        .iter()
        .fold(HashMap::<String, usize>::new(), |mut acc, phase| {
            *acc.entry(phase.label.to_string()).or_insert(0) += 1;
            acc
        })
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .and_then(|(label, _)| parse_phase_label(&label))
}

fn area_band_total(
    counts: &HashMap<(TeamId, PitchArea), usize>,
    team: TeamId,
    areas: &[PitchArea],
) -> usize {
    areas
        .iter()
        .map(|area| *counts.get(&(team, *area)).unwrap_or(&0))
        .sum()
}

fn avg_x(points: &[Point2D]) -> f64 {
    points.iter().map(|p| p.x).sum::<f64>() / points.len().max(1) as f64
}

fn parse_phase_label(label: &str) -> Option<PhaseLabel> {
    match label {
        "Build-up" => Some(PhaseLabel::BuildUp),
        "Progression" => Some(PhaseLabel::Progression),
        "Final Third" => Some(PhaseLabel::FinalThird),
        "Defensive Block" => Some(PhaseLabel::DefensiveBlock),
        "High Press" => Some(PhaseLabel::HighPress),
        "Attacking Transition" => Some(PhaseLabel::AttackingTransition),
        "Defensive Transition" => Some(PhaseLabel::DefensiveTransition),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_line_metrics_ignores_non_finite_points() {
        let mut points = vec![
            Point2D { x: 10.0, y: 10.0 },
            Point2D { x: 20.0, y: 12.0 },
            Point2D {
                x: f64::NAN,
                y: 15.0,
            },
            Point2D { x: 30.0, y: 18.0 },
            Point2D { x: 40.0, y: 20.0 },
            Point2D { x: 50.0, y: 22.0 },
            Point2D { x: 60.0, y: 24.0 },
        ];

        let result = compute_line_metrics(TeamId::TeamA, &mut points);

        assert!(result.is_some());
    }
}
