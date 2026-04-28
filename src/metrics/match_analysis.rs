// metrics/match_analysis.rs — Coach-focused match plan analytics:
// possession time/location/switches, crossing events,
// running/sprint/high-speed metrics, weakest-player ranking.

use super::coach::TeamId;
use super::{PlayerMetrics, TimedPosition};
use crate::detection::COCO_SPORTS_BALL;
use crate::pitch_mapping::{
    pitch_distance, PitchMapper, Point2D, PENALTY_AREA_DEPTH, PENALTY_AREA_WIDTH, PITCH_LENGTH,
    PITCH_WIDTH,
};
use crate::tracker::TrackingResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Running thresholds (standard football analytics) ---
const HIGH_SPEED_RUN_MS: f64 = 5.5; // ≈ 19.8 km/h
const SPRINT_SPEED_MS: f64 = 7.0; // ≈ 25.2 km/h
const SPRINT_MIN_DURATION_S: f64 = 1.0;
const TELEPORT_MS: f64 = 15.0; // drop larger jumps as noise

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchAnalysis {
    pub possession: PossessionAnalysis,
    pub crossing: CrossingAnalysis,
    pub running: RunningAnalysis,
    pub weakest_players: Vec<WeakPlayerScore>,
}

// ---------------- POSSESSION ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchThird {
    Defensive,
    Middle,
    Attacking,
}

impl std::fmt::Display for PitchThird {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PitchThird::Defensive => write!(f, "Defensive Third"),
            PitchThird::Middle => write!(f, "Middle Third"),
            PitchThird::Attacking => write!(f, "Attacking Third"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchLane {
    Left,
    Central,
    Right,
}

impl std::fmt::Display for PitchLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PitchLane::Left => write!(f, "Left"),
            PitchLane::Central => write!(f, "Central"),
            PitchLane::Right => write!(f, "Right"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamPossession {
    pub team: TeamId,
    pub time_secs: f64,
    pub share_pct: f64,
    pub third_time_secs: HashMap<PitchThird, f64>,
    pub lane_time_secs: HashMap<PitchLane, f64>,
    pub avg_switch_time_secs: f64,
    pub switch_count: usize,
    pub longest_same_side_stretch_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PossessionAnalysis {
    pub total_sampled_secs: f64,
    pub teams: Vec<TeamPossession>,
}

fn classify_third(p: &Point2D) -> PitchThird {
    // Relative to Team A attacking right (+x). We just use raw zones here;
    // the coach view already shows team labels, so thirds are a geometric hint.
    if p.x < PITCH_LENGTH / 3.0 {
        PitchThird::Defensive
    } else if p.x < PITCH_LENGTH * 2.0 / 3.0 {
        PitchThird::Middle
    } else {
        PitchThird::Attacking
    }
}

fn classify_lane(p: &Point2D) -> PitchLane {
    if p.y < PITCH_WIDTH / 3.0 {
        PitchLane::Left
    } else if p.y < PITCH_WIDTH * 2.0 / 3.0 {
        PitchLane::Central
    } else {
        PitchLane::Right
    }
}

fn compute_possession(contexts: &[crate::metrics::coach::PossessionContext]) -> PossessionAnalysis {
    if contexts.is_empty() {
        return PossessionAnalysis {
            total_sampled_secs: 0.0,
            teams: Vec::new(),
        };
    }

    // Estimate average sampling interval between consecutive contexts
    let mut intervals: Vec<f64> = contexts
        .windows(2)
        .map(|w| (w[1].timestamp_secs - w[0].timestamp_secs).max(0.0))
        .filter(|dt| dt.is_finite() && *dt > 0.0 && *dt < 10.0)
        .collect();
    intervals.sort_by(|a, b| a.total_cmp(b));
    let avg_interval = if intervals.is_empty() {
        1.0
    } else {
        intervals[intervals.len() / 2]
    };

    let total_sampled_secs = contexts.len() as f64 * avg_interval;

    let mut by_team: HashMap<TeamId, Vec<&crate::metrics::coach::PossessionContext>> =
        HashMap::new();
    for ctx in contexts {
        by_team.entry(ctx.team_in_possession).or_default().push(ctx);
    }

    let mut teams = Vec::new();
    for team in [TeamId::TeamA, TeamId::TeamB] {
        let Some(list) = by_team.get(&team) else {
            continue;
        };
        let time_secs = list.len() as f64 * avg_interval;
        let mut third_time: HashMap<PitchThird, f64> = HashMap::new();
        let mut lane_time: HashMap<PitchLane, f64> = HashMap::new();
        for ctx in list {
            *third_time
                .entry(classify_third(&ctx.ball_position))
                .or_insert(0.0) += avg_interval;
            *lane_time
                .entry(classify_lane(&ctx.ball_position))
                .or_insert(0.0) += avg_interval;
        }

        // Switches of play: consecutive same-team contexts where the lane flips
        // left<->right (ignoring central). Track time between those flips.
        let mut switch_times: Vec<f64> = Vec::new();
        let mut longest_same_side = 0.0_f64;
        let mut last_flank: Option<(PitchLane, f64)> = None;
        let mut same_side_start: Option<f64> = None;
        let mut prev_ts = list.first().map(|c| c.timestamp_secs).unwrap_or(0.0);

        for ctx in list {
            let lane = classify_lane(&ctx.ball_position);
            if lane == PitchLane::Central {
                prev_ts = ctx.timestamp_secs;
                continue;
            }
            match last_flank {
                Some((prev_lane, ts)) if prev_lane != lane => {
                    let dt = (ctx.timestamp_secs - ts).max(0.0);
                    if dt.is_finite() && dt < 30.0 {
                        switch_times.push(dt);
                    }
                    if let Some(ss) = same_side_start {
                        longest_same_side = longest_same_side.max(ts - ss);
                    }
                    same_side_start = Some(ctx.timestamp_secs);
                }
                None => {
                    same_side_start = Some(ctx.timestamp_secs);
                }
                _ => {}
            }
            last_flank = Some((lane, ctx.timestamp_secs));
            prev_ts = ctx.timestamp_secs;
        }
        let _ = prev_ts;
        if let (Some(ss), Some((_, last_ts))) = (same_side_start, last_flank) {
            longest_same_side = longest_same_side.max(last_ts - ss);
        }

        let avg_switch = if switch_times.is_empty() {
            0.0
        } else {
            switch_times.iter().sum::<f64>() / switch_times.len() as f64
        };

        teams.push(TeamPossession {
            team,
            time_secs,
            share_pct: if total_sampled_secs > 0.0 {
                time_secs / total_sampled_secs * 100.0
            } else {
                0.0
            },
            third_time_secs: third_time,
            lane_time_secs: lane_time,
            avg_switch_time_secs: avg_switch,
            switch_count: switch_times.len(),
            longest_same_side_stretch_secs: longest_same_side.max(0.0),
        });
    }

    PossessionAnalysis {
        total_sampled_secs,
        teams,
    }
}

// ---------------- CROSSING ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossSide {
    Left,
    Right,
}

impl std::fmt::Display for CrossSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrossSide::Left => write!(f, "Left"),
            CrossSide::Right => write!(f, "Right"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoxZone {
    NearPost,
    SixYard,
    PenaltySpot,
    FarPost,
    EdgeOfBox,
}

impl std::fmt::Display for BoxZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoxZone::NearPost => write!(f, "Near post"),
            BoxZone::SixYard => write!(f, "6-yard"),
            BoxZone::PenaltySpot => write!(f, "Penalty spot"),
            BoxZone::FarPost => write!(f, "Far post"),
            BoxZone::EdgeOfBox => write!(f, "Edge of box"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossEvent {
    pub timestamp_secs: f64,
    pub attacking_team: TeamId,
    pub side: CrossSide,
    pub origin: Point2D,
    pub delivery_point: Point2D,
    pub attackers_in_box: usize,
    pub defenders_in_box: usize,
    pub attacker_zones: Vec<BoxZone>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossingAnalysis {
    pub events: Vec<CrossEvent>,
    pub by_team: HashMap<TeamId, usize>,
}

fn box_zone_for(p: &Point2D, attacking_right: bool) -> Option<BoxZone> {
    // Attacking right means goal is at x = PITCH_LENGTH.
    let x_goal = if attacking_right { PITCH_LENGTH } else { 0.0 };
    let dist_from_goal_line = if attacking_right {
        PITCH_LENGTH - p.x
    } else {
        p.x
    };
    if dist_from_goal_line < 0.0 || dist_from_goal_line > PENALTY_AREA_DEPTH + 2.0 {
        return None;
    }
    let half_box_y = PENALTY_AREA_WIDTH / 2.0;
    let y_off = p.y - PITCH_WIDTH / 2.0;
    if y_off.abs() > half_box_y + 1.0 {
        return None;
    }

    // Edge of box: last ~4m of penalty area depth
    if dist_from_goal_line > PENALTY_AREA_DEPTH - 4.0 {
        return Some(BoxZone::EdgeOfBox);
    }
    // 6-yard area: within 5.5m of goal line
    if dist_from_goal_line <= 5.5 && y_off.abs() <= 9.16 {
        return Some(BoxZone::SixYard);
    }
    // Penalty spot region: central, around 11m
    if y_off.abs() < 7.0 && (dist_from_goal_line - 11.0).abs() < 4.0 {
        return Some(BoxZone::PenaltySpot);
    }
    // Near vs far post determined by side of cross origin — caller decides,
    // here we tag by y relative to center.
    if (attacking_right && y_off < 0.0) || (!attacking_right && y_off < 0.0) {
        Some(BoxZone::NearPost)
    } else {
        Some(BoxZone::FarPost)
    }
    .and_then(|z| {
        let _ = x_goal;
        Some(z)
    })
}

fn compute_crossing(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    team_assignments: &HashMap<u32, TeamId>,
) -> CrossingAnalysis {
    // Team attacking direction: majority of team players' avg_x vs half-line.
    // TeamA attacks +x if their mean x > PITCH_LENGTH/2? Instead, count deepest
    // defender side per team across samples.
    let attacking_right = infer_attacking_direction(tracking, mapper, team_assignments);

    let mut events: Vec<CrossEvent> = Vec::new();
    let mut by_team: HashMap<TeamId, usize> = HashMap::new();

    let mut last_wide_advanced: Option<(f64, TeamId, Point2D, CrossSide)> = None;
    let cooldown_secs = 4.0;
    let mut last_event_time: f64 = f64::NEG_INFINITY;

    for ft in &tracking.frame_tracks {
        let Some(ball) = ft.tracks.iter().find(|t| t.class_id == COCO_SPORTS_BALL) else {
            continue;
        };
        let ball_pos = mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);
        if !ball_pos.x.is_finite() || !ball_pos.y.is_finite() {
            continue;
        }

        // Find closest player to ball -> possessing team
        let mut closest: Option<(u32, f64)> = None;
        for t in &ft.tracks {
            if t.class_name != "Player" {
                continue;
            }
            let p = mapper.bbox_to_pitch(t.bbox.x1, t.bbox.y1, t.bbox.x2, t.bbox.y2);
            if !p.x.is_finite() || !p.y.is_finite() {
                continue;
            }
            let d = pitch_distance(&ball_pos, &p);
            if !d.is_finite() {
                continue;
            }
            if closest.map_or(true, |(_, cd)| d < cd) {
                closest = Some((t.track_id, d));
            }
        }
        let Some((carrier_id, _)) = closest else {
            continue;
        };
        let Some(&team) = team_assignments.get(&carrier_id) else {
            continue;
        };

        let atk_right = *attacking_right.get(&team).unwrap_or(&true);
        let x_rel = if atk_right {
            ball_pos.x
        } else {
            PITCH_LENGTH - ball_pos.x
        };
        let y_off = ball_pos.y - PITCH_WIDTH / 2.0;

        // Wide advanced: advanced third and flank
        let wide_advanced = x_rel > PITCH_LENGTH * 0.70 && y_off.abs() > 16.0;
        if wide_advanced {
            let side = if y_off < 0.0 {
                CrossSide::Left
            } else {
                CrossSide::Right
            };
            last_wide_advanced = Some((ft.timestamp_secs, team, ball_pos, side));
            continue;
        }

        // Cross completion: ball enters box for same team within 2.5s of a wide advanced sample
        if let Some((origin_ts, origin_team, origin_pos, side)) = last_wide_advanced {
            if team != origin_team || ft.timestamp_secs - origin_ts > 2.5 {
                continue;
            }
            let atk_right = *attacking_right.get(&team).unwrap_or(&true);
            let dist_from_goal = if atk_right {
                PITCH_LENGTH - ball_pos.x
            } else {
                ball_pos.x
            };
            let in_box = dist_from_goal >= 0.0
                && dist_from_goal <= PENALTY_AREA_DEPTH
                && y_off.abs() <= PENALTY_AREA_WIDTH / 2.0;
            if !in_box {
                continue;
            }
            if ft.timestamp_secs - last_event_time < cooldown_secs {
                continue;
            }
            last_event_time = ft.timestamp_secs;

            // Count attackers vs defenders in box, and attacker zones
            let mut attackers = 0usize;
            let mut defenders = 0usize;
            let mut attacker_zones = Vec::new();
            for t in &ft.tracks {
                if t.class_name != "Player" {
                    continue;
                }
                let Some(&t_team) = team_assignments.get(&t.track_id) else {
                    continue;
                };
                let p = mapper.bbox_to_pitch(t.bbox.x1, t.bbox.y1, t.bbox.x2, t.bbox.y2);
                if !p.x.is_finite() || !p.y.is_finite() {
                    continue;
                }
                let Some(zone) = box_zone_for(&p, atk_right) else {
                    continue;
                };
                if t_team == team {
                    attackers += 1;
                    attacker_zones.push(zone);
                } else {
                    defenders += 1;
                }
            }

            let event = CrossEvent {
                timestamp_secs: ft.timestamp_secs,
                attacking_team: team,
                side,
                origin: origin_pos,
                delivery_point: ball_pos,
                attackers_in_box: attackers,
                defenders_in_box: defenders,
                attacker_zones,
            };
            *by_team.entry(team).or_insert(0) += 1;
            events.push(event);
            last_wide_advanced = None;
        }
    }

    CrossingAnalysis { events, by_team }
}

fn infer_attacking_direction(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    team_assignments: &HashMap<u32, TeamId>,
) -> HashMap<TeamId, bool> {
    let mut sum_x: HashMap<TeamId, (f64, u64)> = HashMap::new();
    for ft in tracking.frame_tracks.iter().step_by(8).take(200) {
        for t in &ft.tracks {
            if t.class_name != "Player" {
                continue;
            }
            let Some(&team) = team_assignments.get(&t.track_id) else {
                continue;
            };
            let p = mapper.bbox_to_pitch(t.bbox.x1, t.bbox.y1, t.bbox.x2, t.bbox.y2);
            if !p.x.is_finite() {
                continue;
            }
            let e = sum_x.entry(team).or_insert((0.0, 0));
            e.0 += p.x;
            e.1 += 1;
        }
    }
    let mut out = HashMap::new();
    // The team with higher average x attacks +x (right); the other attacks left.
    let means: Vec<(TeamId, f64)> = sum_x
        .iter()
        .map(|(t, (s, n))| {
            (
                *t,
                if *n > 0 {
                    s / *n as f64
                } else {
                    PITCH_LENGTH / 2.0
                },
            )
        })
        .collect();
    if means.len() == 2 {
        let (a, b) = (means[0], means[1]);
        if a.1 >= b.1 {
            out.insert(a.0, true);
            out.insert(b.0, false);
        } else {
            out.insert(a.0, false);
            out.insert(b.0, true);
        }
    } else {
        for (t, _) in means {
            out.insert(t, true);
        }
    }
    out
}

// ---------------- RUNNING ----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRunning {
    pub track_id: u32,
    pub team: Option<TeamId>,
    pub total_distance_m: f64,
    pub high_speed_run_secs: f64,
    pub high_speed_distance_m: f64,
    pub sprint_count: u32,
    pub max_speed_kmh: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRunning {
    pub team: TeamId,
    pub total_distance_m: f64,
    pub high_speed_run_secs: f64,
    pub sprint_count: u32,
    pub players_counted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningAnalysis {
    pub players: Vec<PlayerRunning>,
    pub teams: Vec<TeamRunning>,
}

fn compute_running(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    team_assignments: &HashMap<u32, TeamId>,
    player_metrics: &HashMap<u32, PlayerMetrics>,
) -> RunningAnalysis {
    let mut players = Vec::new();

    for (&track_id, positions) in player_positions {
        if positions.len() < 3 {
            continue;
        }
        let mut total_distance = 0.0_f64;
        let mut hsr_secs = 0.0_f64;
        let mut hsr_dist = 0.0_f64;
        let mut sprint_count = 0u32;
        let mut in_sprint = false;
        let mut sprint_run_start: f64 = 0.0;

        for w in positions.windows(2) {
            let dt = (w[1].timestamp_secs - w[0].timestamp_secs).max(0.0);
            if !dt.is_finite() || dt <= 0.0 || dt > 2.0 {
                continue;
            }
            let dx = w[1].pitch_pos.x - w[0].pitch_pos.x;
            let dy = w[1].pitch_pos.y - w[0].pitch_pos.y;
            let d = (dx * dx + dy * dy).sqrt();
            if !d.is_finite() {
                continue;
            }
            let speed = d / dt;
            if !speed.is_finite() || speed > TELEPORT_MS {
                continue;
            }
            total_distance += d;
            if speed >= HIGH_SPEED_RUN_MS {
                hsr_secs += dt;
                hsr_dist += d;
            }
            if speed >= SPRINT_SPEED_MS {
                if !in_sprint {
                    in_sprint = true;
                    sprint_run_start = w[0].timestamp_secs;
                }
                if w[1].timestamp_secs - sprint_run_start >= SPRINT_MIN_DURATION_S {
                    // count once and require a break before next
                    sprint_count += 1;
                    in_sprint = false;
                }
            } else if in_sprint {
                in_sprint = false;
            }
        }

        let max_speed_kmh = player_metrics
            .get(&track_id)
            .map(|pm| pm.max_speed_kmh)
            .unwrap_or(0.0);

        players.push(PlayerRunning {
            track_id,
            team: team_assignments.get(&track_id).copied(),
            total_distance_m: total_distance,
            high_speed_run_secs: hsr_secs,
            high_speed_distance_m: hsr_dist,
            sprint_count,
            max_speed_kmh,
        });
    }

    players.sort_by(|a, b| b.total_distance_m.total_cmp(&a.total_distance_m));

    let mut teams_map: HashMap<TeamId, TeamRunning> = HashMap::new();
    for p in &players {
        let Some(team) = p.team else {
            continue;
        };
        let entry = teams_map.entry(team).or_insert(TeamRunning {
            team,
            total_distance_m: 0.0,
            high_speed_run_secs: 0.0,
            sprint_count: 0,
            players_counted: 0,
        });
        entry.total_distance_m += p.total_distance_m;
        entry.high_speed_run_secs += p.high_speed_run_secs;
        entry.sprint_count += p.sprint_count;
        entry.players_counted += 1;
    }
    let mut teams: Vec<_> = teams_map.into_values().collect();
    teams.sort_by(|a, b| b.total_distance_m.total_cmp(&a.total_distance_m));

    RunningAnalysis { players, teams }
}

// ---------------- WEAKEST PLAYER ----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeakPlayerScore {
    pub track_id: u32,
    pub team: Option<TeamId>,
    pub weakness: f64, // 0..1, higher = weaker
    pub low_speed_factor: f64,
    pub low_activity_factor: f64,
    pub turnover_factor: f64,
    pub duel_loss_factor: f64,
    pub notes: String,
}

fn compute_weakest_players(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    player_metrics: &HashMap<u32, PlayerMetrics>,
    team_assignments: &HashMap<u32, TeamId>,
    running: &RunningAnalysis,
    tracking: &TrackingResult,
    mapper: &PitchMapper,
) -> Vec<WeakPlayerScore> {
    // Turnovers: when closest-to-ball carrier changes team, attribute turnover
    // to the previous carrier.
    let mut turnovers: HashMap<u32, u32> = HashMap::new();
    let mut duel_losses: HashMap<u32, u32> = HashMap::new();
    let mut prev_carrier: Option<(u32, TeamId)> = None;

    for ft in &tracking.frame_tracks {
        let Some(ball) = ft.tracks.iter().find(|t| t.class_id == COCO_SPORTS_BALL) else {
            continue;
        };
        let ball_pos = mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);
        if !ball_pos.x.is_finite() {
            continue;
        }
        let mut closest: Option<(u32, TeamId, f64)> = None;
        for t in &ft.tracks {
            if t.class_name != "Player" {
                continue;
            }
            let Some(&team) = team_assignments.get(&t.track_id) else {
                continue;
            };
            let p = mapper.bbox_to_pitch(t.bbox.x1, t.bbox.y1, t.bbox.x2, t.bbox.y2);
            if !p.x.is_finite() {
                continue;
            }
            let d = pitch_distance(&ball_pos, &p);
            if !d.is_finite() {
                continue;
            }
            if closest.map_or(true, |(_, _, cd)| d < cd) {
                closest = Some((t.track_id, team, d));
            }
        }
        if let Some((carrier_id, carrier_team, carrier_dist)) = closest {
            if let Some((prev_id, prev_team)) = prev_carrier {
                if prev_team != carrier_team && prev_id != carrier_id {
                    *turnovers.entry(prev_id).or_insert(0) += 1;
                    // 1v1 defense loss: if carrier_dist <= 3m and there was a
                    // defender within ~3m last frame — attribute duel loss to
                    // nearest opponent to the new carrier this frame.
                    if carrier_dist <= 3.0 {
                        let mut nearest_opp: Option<(u32, f64)> = None;
                        for t in &ft.tracks {
                            if t.class_name != "Player" {
                                continue;
                            }
                            if t.track_id == carrier_id {
                                continue;
                            }
                            let Some(&tt) = team_assignments.get(&t.track_id) else {
                                continue;
                            };
                            if tt == carrier_team {
                                continue;
                            }
                            let p =
                                mapper.bbox_to_pitch(t.bbox.x1, t.bbox.y1, t.bbox.x2, t.bbox.y2);
                            if !p.x.is_finite() {
                                continue;
                            }
                            let d = pitch_distance(&ball_pos, &p);
                            if !d.is_finite() {
                                continue;
                            }
                            if nearest_opp.map_or(true, |(_, cd)| d < cd) {
                                nearest_opp = Some((t.track_id, d));
                            }
                        }
                        if let Some((opp_id, od)) = nearest_opp {
                            if od < 5.0 {
                                *duel_losses.entry(opp_id).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            prev_carrier = Some((carrier_id, carrier_team));
        }
    }

    // Normalize within team for fairness.
    let mut scores: Vec<WeakPlayerScore> = Vec::new();
    let running_by_id: HashMap<u32, &PlayerRunning> =
        running.players.iter().map(|p| (p.track_id, p)).collect();

    // Compute per-team max stats for normalization
    let mut team_max: HashMap<TeamId, (f64, f64, f64, f64)> = HashMap::new();
    for (&tid, pm) in player_metrics {
        let Some(&team) = team_assignments.get(&tid) else {
            continue;
        };
        let dist = running_by_id
            .get(&tid)
            .map(|r| r.total_distance_m)
            .unwrap_or(pm.total_distance_m);
        let avg_speed = pm.avg_speed_kmh.max(0.0);
        let tov = *turnovers.get(&tid).unwrap_or(&0) as f64;
        let dl = *duel_losses.get(&tid).unwrap_or(&0) as f64;
        let e = team_max.entry(team).or_insert((0.0, 0.0, 0.0, 0.0));
        e.0 = e.0.max(avg_speed);
        e.1 = e.1.max(dist);
        e.2 = e.2.max(tov);
        e.3 = e.3.max(dl);
    }

    for (&tid, pm) in player_metrics {
        if !pm.total_distance_m.is_finite() {
            continue;
        }
        let Some(&team) = team_assignments.get(&tid) else {
            continue;
        };
        if !player_positions.get(&tid).map_or(false, |p| p.len() >= 5) {
            continue;
        }

        let dist = running_by_id
            .get(&tid)
            .map(|r| r.total_distance_m)
            .unwrap_or(pm.total_distance_m);
        let avg_speed = pm.avg_speed_kmh.max(0.0);
        let tov = *turnovers.get(&tid).unwrap_or(&0) as f64;
        let dl = *duel_losses.get(&tid).unwrap_or(&0) as f64;

        let (max_speed_t, max_dist_t, max_tov_t, max_dl_t) =
            team_max.get(&team).copied().unwrap_or((1.0, 1.0, 1.0, 1.0));

        let low_speed_factor = 1.0 - (avg_speed / max_speed_t.max(0.1)).clamp(0.0, 1.0);
        let low_activity_factor = 1.0 - (dist / max_dist_t.max(1.0)).clamp(0.0, 1.0);
        let turnover_factor = (tov / max_tov_t.max(1.0)).clamp(0.0, 1.0);
        let duel_loss_factor = (dl / max_dl_t.max(1.0)).clamp(0.0, 1.0);

        // Weights: activity 0.3, speed 0.2, turnovers 0.25, duel losses 0.25
        let weakness = 0.30 * low_activity_factor
            + 0.20 * low_speed_factor
            + 0.25 * turnover_factor
            + 0.25 * duel_loss_factor;

        let mut notes = Vec::new();
        if low_activity_factor > 0.6 {
            notes.push("low total distance".to_string());
        }
        if low_speed_factor > 0.6 {
            notes.push("slow average speed".to_string());
        }
        if turnover_factor > 0.5 {
            notes.push(format!("{} turnovers", tov as u32));
        }
        if duel_loss_factor > 0.5 {
            notes.push(format!("{} duels lost", dl as u32));
        }

        scores.push(WeakPlayerScore {
            track_id: tid,
            team: Some(team),
            weakness,
            low_speed_factor,
            low_activity_factor,
            turnover_factor,
            duel_loss_factor,
            notes: if notes.is_empty() {
                "balanced profile".to_string()
            } else {
                notes.join(", ")
            },
        });
    }

    scores.sort_by(|a, b| b.weakness.total_cmp(&a.weakness));
    scores
}

// ---------------- ENTRY POINT ----------------

pub fn compute_match_analysis(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    player_metrics: &HashMap<u32, PlayerMetrics>,
    coach_metrics: &crate::metrics::coach::CoachMetrics,
) -> MatchAnalysis {
    let possession = compute_possession(&coach_metrics.possession_contexts);
    let crossing = compute_crossing(tracking, mapper, &coach_metrics.team_assignments);
    let running = compute_running(
        player_positions,
        &coach_metrics.team_assignments,
        player_metrics,
    );
    let weakest_players = compute_weakest_players(
        player_positions,
        player_metrics,
        &coach_metrics.team_assignments,
        &running,
        tracking,
        mapper,
    );

    MatchAnalysis {
        possession,
        crossing,
        running,
        weakest_players,
    }
}
