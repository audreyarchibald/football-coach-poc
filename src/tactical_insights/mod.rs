// tactical_insights/mod.rs — Generate coach-friendly text insights

use crate::metrics::coach::team_label;
use crate::metrics::pressing::PitchZone;
use crate::metrics::{ClipMetrics, PlayerMetrics};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A tactical insight with category and text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TacticalInsight {
    pub category: InsightCategory,
    pub title: String,
    pub description: String,
    pub importance: Importance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsightCategory {
    Pressing,
    Possession,
    Movement,
    Formation,
    General,
}

impl std::fmt::Display for InsightCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InsightCategory::Pressing => write!(f, "Pressing"),
            InsightCategory::Possession => write!(f, "Possession"),
            InsightCategory::Movement => write!(f, "Movement"),
            InsightCategory::Formation => write!(f, "Formation"),
            InsightCategory::General => write!(f, "General"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Importance {
    High,
    Medium,
    Low,
}

/// Generate tactical insights from computed metrics
pub fn generate_insights(metrics: &ClipMetrics) -> Vec<TacticalInsight> {
    let mut insights = Vec::new();

    // --- Pressing insights ---
    generate_pressing_insights(metrics, &mut insights);

    // --- Possession insights ---
    generate_possession_insights(metrics, &mut insights);

    // --- Movement insights ---
    generate_movement_insights(metrics, &mut insights);

    // --- Spatial insights ---
    generate_area_insights(metrics, &mut insights);

    // --- Coach insights ---
    generate_coach_insights(metrics, &mut insights);

    // --- General clip info ---
    insights.push(TacticalInsight {
        category: InsightCategory::General,
        title: "Clip Summary".to_string(),
        description: format!(
            "Analyzed {:.1}s of play ({} frames). Tracked {} players.",
            metrics.duration_secs,
            metrics.total_frames,
            metrics.player_metrics.len(),
        ),
        importance: Importance::Low,
    });

    // Sort by importance
    insights.sort_by(|a, b| {
        let ord = |imp: &Importance| match imp {
            Importance::High => 0,
            Importance::Medium => 1,
            Importance::Low => 2,
        };
        ord(&a.importance).cmp(&ord(&b.importance))
    });

    log::info!("Generated {} tactical insights", insights.len());
    insights
}

fn generate_pressing_insights(metrics: &ClipMetrics, insights: &mut Vec<TacticalInsight>) {
    if metrics.pressing_events.is_empty() {
        return;
    }

    // Count pressing events by zone
    let mut zone_counts: HashMap<String, usize> = HashMap::new();
    let mut total_intensity: f32 = 0.0;
    let mut high_press_count = 0usize;

    for event in &metrics.pressing_events {
        *zone_counts.entry(event.zone.to_string()).or_insert(0) += 1;
        total_intensity += event.intensity;
        if event.zone == PitchZone::AttackingThird {
            high_press_count += 1;
        }
    }

    let avg_intensity = total_intensity / metrics.pressing_events.len() as f32;

    // Find most active pressing zone
    if let Some((zone, count)) = zone_counts.iter().max_by_key(|(_, c)| **c) {
        let pct = *count as f64 / metrics.pressing_events.len() as f64 * 100.0;
        insights.push(TacticalInsight {
            category: InsightCategory::Pressing,
            title: format!("Pressing concentration: {}", zone),
            description: format!(
                "Most pressing activity in the {} ({:.0}% of {} events, avg intensity {:.0}%).",
                zone,
                pct,
                metrics.pressing_events.len(),
                avg_intensity * 100.0
            ),
            importance: Importance::High,
        });
    }

    if high_press_count > 0 {
        insights.push(TacticalInsight {
            category: InsightCategory::Pressing,
            title: "High pressing detected".to_string(),
            description: format!(
                "Detected {} high-press moments in the attacking third over {:.1}s.",
                high_press_count, metrics.duration_secs
            ),
            importance: Importance::High,
        });
    }
}

fn generate_possession_insights(metrics: &ClipMetrics, insights: &mut Vec<TacticalInsight>) {
    if metrics.ball_possessions.is_empty() {
        insights.push(TacticalInsight {
            category: InsightCategory::Possession,
            title: "Ball not consistently tracked".to_string(),
            description: "Ball detection was limited. Possession data may be incomplete."
                .to_string(),
            importance: Importance::Medium,
        });
        return;
    }

    // Count touches per player
    let mut touches: HashMap<u32, usize> = HashMap::new();
    for event in &metrics.ball_possessions {
        *touches.entry(event.player_track_id).or_insert(0) += 1;
    }

    // Find most active player
    if let Some((&player_id, &count)) = touches.iter().max_by_key(|(_, c)| **c) {
        insights.push(TacticalInsight {
            category: InsightCategory::Possession,
            title: format!("Most touches: Player #{}", player_id),
            description: format!(
                "Player #{} had the most ball involvement with {} proximity events.",
                player_id, count
            ),
            importance: Importance::Medium,
        });
    }

    insights.push(TacticalInsight {
        category: InsightCategory::Possession,
        title: "Ball involvement".to_string(),
        description: format!(
            "{} total touch events across {} players.",
            metrics.ball_possessions.len(),
            touches.len()
        ),
        importance: Importance::Low,
    });
}

fn generate_movement_insights(metrics: &ClipMetrics, insights: &mut Vec<TacticalInsight>) {
    if metrics.player_metrics.is_empty() {
        return;
    }

    // Find fastest and most active players
    let mut players: Vec<&PlayerMetrics> = metrics.player_metrics.values().collect();

    // Most distance covered
    players
        .retain(|player| player.total_distance_m.is_finite() && player.max_speed_kmh.is_finite());
    if players.is_empty() {
        return;
    }

    players.sort_by(|a, b| b.total_distance_m.total_cmp(&a.total_distance_m));
    if let Some(top) = players.first() {
        insights.push(TacticalInsight {
            category: InsightCategory::Movement,
            title: format!("Most distance: Player #{}", top.track_id),
            description: format!(
                "Player #{} covered {:.1}m (avg {:.1} km/h, max {:.1} km/h).",
                top.track_id, top.total_distance_m, top.avg_speed_kmh, top.max_speed_kmh
            ),
            importance: Importance::Medium,
        });
    }

    // Highest top speed
    players.sort_by(|a, b| b.max_speed_kmh.total_cmp(&a.max_speed_kmh));
    if let Some(fastest) = players.first() {
        if fastest.max_speed_kmh > 15.0 {
            insights.push(TacticalInsight {
                category: InsightCategory::Movement,
                title: format!("Sprint detected: Player #{}", fastest.track_id),
                description: format!(
                    "Player #{} reached {:.1} km/h — indicates a sprint/counter-attack.",
                    fastest.track_id, fastest.max_speed_kmh
                ),
                importance: Importance::High,
            });
        }
    }

    // Average team movement
    let total_dist: f64 = players.iter().map(|p| p.total_distance_m).sum();
    let avg_dist = total_dist / players.len() as f64;
    insights.push(TacticalInsight {
        category: InsightCategory::Movement,
        title: "Team movement summary".to_string(),
        description: format!(
            "{} tracked players covered {:.0}m total (avg {:.1}m per player).",
            players.len(),
            total_dist,
            avg_dist
        ),
        importance: Importance::Low,
    });
}

fn generate_area_insights(metrics: &ClipMetrics, insights: &mut Vec<TacticalInsight>) {
    if metrics.dominant_areas.is_empty() {
        return;
    }

    let mut area_counts: HashMap<String, usize> = HashMap::new();
    for area in metrics.dominant_areas.values() {
        *area_counts.entry(area.to_string()).or_insert(0) += 1;
    }

    if let Some((area, count)) = area_counts.iter().max_by_key(|(_, count)| **count) {
        insights.push(TacticalInsight {
            category: InsightCategory::Formation,
            title: format!("Main occupation zone: {}", area),
            description: format!(
                "{} tracked players spent most of their time in the {}. Useful for spotting where the phase of play is happening.",
                count, area
            ),
            importance: Importance::Medium,
        });
    }

    if let Some((&track_id, area)) = metrics.dominant_areas.iter().next() {
        insights.push(TacticalInsight {
            category: InsightCategory::Formation,
            title: format!("Player #{} operating zone", track_id),
            description: format!(
                "Player #{} is spending most of the clip in the {}.",
                track_id, area
            ),
            importance: Importance::Low,
        });
    }
}

fn generate_coach_insights(metrics: &ClipMetrics, insights: &mut Vec<TacticalInsight>) {
    if let Some(phase) = &metrics.coach_metrics.dominant_phase {
        insights.push(TacticalInsight {
            category: InsightCategory::Formation,
            title: format!("Main phase: {}", phase),
            description: format!(
                "Most of this clip looks like {}. Useful for filtering clips by game phase instead of by highlight moment.",
                phase
            ),
            importance: Importance::High,
        });
    }

    for shape in metrics.coach_metrics.team_shapes.values() {
        insights.push(TacticalInsight {
            category: InsightCategory::Formation,
            title: format!("{} shape", team_label(shape.team, &metrics.coach_metrics)),
            description: format!(
                "{} average width {:.1}m, depth {:.1}m, line height {:.1}m. This is the structural picture a coach wants before judging individuals.",
                team_label(shape.team, &metrics.coach_metrics), shape.width_m, shape.depth_m, shape.line_height_m
            ),
            importance: Importance::Medium,
        });
    }

    for alert in metrics.coach_metrics.structural_alerts.iter().take(4) {
        insights.push(TacticalInsight {
            category: InsightCategory::Formation,
            title: format!("{}: {}", team_label(alert.team, &metrics.coach_metrics), alert.title),
            description: alert.description.clone(),
            importance: if alert.severity > 0.75 {
                Importance::High
            } else {
                Importance::Medium
            },
        });
    }

    if !metrics.coach_metrics.frame_team_shapes.is_empty() {
        let weakest_rest = metrics
            .coach_metrics
            .frame_team_shapes
            .iter()
            .filter(|sample| sample.rest_defense_score.is_finite())
            .min_by(|a, b| a.rest_defense_score.total_cmp(&b.rest_defense_score));
        if let Some(sample) = weakest_rest {
            insights.push(TacticalInsight {
                category: InsightCategory::Formation,
                title: format!("{} rest defense sample", team_label(sample.team, &metrics.coach_metrics)),
                description: format!(
                    "At {:.1}s, {} rest-defense score is {:.0}%. This helps flag whether the team is protected behind the ball when attacking.",
                    sample.timestamp_secs,
                    team_label(sample.team, &metrics.coach_metrics),
                    sample.rest_defense_score * 100.0,
                ),
                importance: if sample.rest_defense_score < 0.35 {
                    Importance::High
                } else {
                    Importance::Medium
                },
            });
        }
    }

    for lines in metrics.coach_metrics.team_lines.values() {
        if lines.back_to_mid_spacing_m > 16.0 {
            insights.push(TacticalInsight {
                category: InsightCategory::Formation,
                title: format!("{} back-to-mid gap", team_label(lines.team, &metrics.coach_metrics)),
                description: format!(
                    "{} show {:.1}m between back line and midfield. That is a coach-level warning sign for vertical disconnection.",
                    team_label(lines.team, &metrics.coach_metrics), lines.back_to_mid_spacing_m
                ),
                importance: Importance::High,
            });
        }

        if lines.between_lines_occupation_score < 0.35 {
            insights.push(TacticalInsight {
                category: InsightCategory::Formation,
                title: format!("{} lack between-lines presence", team_label(lines.team, &metrics.coach_metrics)),
                description: format!(
                    "{} only score {:.0}% for between-lines occupation. The team may be circulating around the block instead of breaking it.",
                    team_label(lines.team, &metrics.coach_metrics),
                    lines.between_lines_occupation_score * 100.0,
                ),
                importance: Importance::High,
            });
        }
    }

    if !metrics.coach_metrics.possession_contexts.is_empty() {
        let weak_support = metrics
            .coach_metrics
            .possession_contexts
            .iter()
            .filter(|ctx| ctx.support_near_ball < 2)
            .count();
        let no_central_receiver = metrics
            .coach_metrics
            .possession_contexts
            .iter()
            .filter(|ctx| !ctx.central_receiver_available)
            .count();

        if weak_support > 0 {
            insights.push(TacticalInsight {
                category: InsightCategory::Formation,
                title: "Ball carrier support is often weak".to_string(),
                description: format!(
                    "{} sampled moments showed fewer than two nearby support options around the ball. This matters for pressure resistance and clean progression.",
                    weak_support
                ),
                importance: Importance::High,
            });
        }

        if no_central_receiver > 0 {
            insights.push(TacticalInsight {
                category: InsightCategory::Formation,
                title: "Central receiver often missing".to_string(),
                description: format!(
                    "{} sampled possession moments had no central receiver option near the ball. The team may be forced wide or backwards too often.",
                    no_central_receiver
                ),
                importance: Importance::High,
            });
        }
    }
}
