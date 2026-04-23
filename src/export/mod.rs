// export/mod.rs — Export tracking data and reports

use crate::metrics::coach::team_label;
use crate::metrics::ClipMetrics;
use crate::tactical_insights::TacticalInsight;
use crate::tracker::TrackingResult;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

/// Full export payload
#[derive(Debug, Serialize)]
pub struct ExportData {
    pub metadata: ExportMetadata,
    pub tracking: TrackingResult,
    pub metrics: ClipMetrics,
    pub insights: Vec<TacticalInsight>,
}

#[derive(Debug, Serialize)]
pub struct ExportMetadata {
    pub video_path: String,
    pub export_time: String,
    pub version: String,
}

/// Export tracking data + metrics to JSON
pub fn export_json(
    path: &Path,
    video_path: &str,
    tracking: &TrackingResult,
    metrics: &ClipMetrics,
    insights: &[TacticalInsight],
) -> Result<()> {
    let data = ExportData {
        metadata: ExportMetadata {
            video_path: video_path.to_string(),
            export_time: chrono::Utc::now().to_rfc3339(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        tracking: tracking.clone(),
        metrics: metrics.clone(),
        insights: insights.to_vec(),
    };

    let json = serde_json::to_string_pretty(&data)?;
    std::fs::write(path, json)?;

    log::info!("Exported JSON report to: {}", path.display());
    Ok(())
}

/// Generate a plain-text tactical summary
pub fn generate_text_report(
    video_path: &str,
    metrics: &ClipMetrics,
    insights: &[TacticalInsight],
) -> String {
    let mut report = String::new();

    report.push_str("═══════════════════════════════════════════════\n");
    report.push_str("     FOOTBALL COACH PoC — TACTICAL REPORT     \n");
    report.push_str("═══════════════════════════════════════════════\n\n");

    report.push_str(&format!("Video: {}\n", video_path));
    report.push_str(&format!(
        "Duration: {:.1}s ({} frames analyzed)\n",
        metrics.duration_secs, metrics.total_frames
    ));
    report.push_str(&format!(
        "Players tracked: {}\n",
        metrics.player_metrics.len()
    ));
    report.push_str(&format!(
        "Generated: {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));

    report.push_str("── KEY INSIGHTS ──────────────────────────────\n\n");

    for (i, insight) in insights.iter().enumerate() {
        let imp = match insight.importance {
            crate::tactical_insights::Importance::High => "!!",
            crate::tactical_insights::Importance::Medium => "! ",
            crate::tactical_insights::Importance::Low => "  ",
        };
        report.push_str(&format!(
            "  {} {}. [{}] {}\n",
            imp,
            i + 1,
            insight.category,
            insight.title
        ));
        report.push_str(&format!("     {}\n\n", insight.description));
    }

    report.push_str("── PLAYER METRICS ────────────────────────────\n\n");
    report.push_str("  ID   | Distance  | Avg Speed  | Max Speed  \n");
    report.push_str("  ─────|───────────|────────────|────────────\n");

    let mut players: Vec<_> = metrics.player_metrics.values().collect();
    players.retain(|player| player.total_distance_m.is_finite());
    players.sort_by(|a, b| b.total_distance_m.total_cmp(&a.total_distance_m));

    for p in &players {
        report.push_str(&format!(
            "  #{:<4}| {:<9.1}m| {:<10.1} | {:<10.1}\n",
            p.track_id, p.total_distance_m, p.avg_speed_kmh, p.max_speed_kmh
        ));
    }

    if !metrics.dominant_areas.is_empty() {
        report.push_str("\n── SPATIAL AWARENESS ─────────────────────────\n\n");
        report.push_str("  ID   | Main Area\n");
        report.push_str("  ─────|────────────────────\n");

        let mut area_rows: Vec<_> = metrics.dominant_areas.iter().collect();
        area_rows.sort_by_key(|(track_id, _)| **track_id);

        for (track_id, area) in area_rows {
            report.push_str(&format!("  #{:<4}| {}\n", track_id, area));
        }
    }

    report.push_str("\n── COACH REVIEW ──────────────────────────────\n\n");
    if let Some(phase) = &metrics.coach_metrics.dominant_phase {
        report.push_str(&format!("Dominant phase: {}\n", phase));
    }
    for shape in metrics.coach_metrics.team_shapes.values() {
        report.push_str(&format!(
            "{} | width {:.1}m | depth {:.1}m | line height {:.1}m | compactness {:.0} m^2\n",
            team_label(shape.team, &metrics.coach_metrics), shape.width_m, shape.depth_m, shape.line_height_m, shape.compactness_m2
        ));
    }

    if !metrics.coach_metrics.team_colors.is_empty() {
        report.push_str("\nTeam colors:\n");
        for (team, color) in &metrics.coach_metrics.team_colors {
            report.push_str(&format!(
                "- {} | rgb {:.2}/{:.2}/{:.2} | sat {:.2}\n",
                team_label(*team, &metrics.coach_metrics), color.r, color.g, color.b, color.saturation
            ));
        }
    }

    if !metrics.coach_metrics.team_lines.is_empty() {
        report.push_str("\nLine / unit analysis:\n");
        for lines in metrics.coach_metrics.team_lines.values() {
            report.push_str(&format!(
                "- {} | back {:.1}m | mid {:.1}m | front {:.1}m | B-M {:.1}m | M-F {:.1}m | between-lines {:.0}%\n",
                team_label(lines.team, &metrics.coach_metrics),
                lines.back_line_height_m,
                lines.midfield_height_m,
                lines.front_line_height_m,
                lines.back_to_mid_spacing_m,
                lines.mid_to_front_spacing_m,
                lines.between_lines_occupation_score * 100.0,
            ));
        }
    }

    if !metrics.coach_metrics.frame_team_shapes.is_empty() {
        report.push_str("\nFrame shape samples:\n");
        for sample in metrics.coach_metrics.frame_team_shapes.iter().take(8) {
            report.push_str(&format!(
                "- {:.1}s | {} | width {:.1}m | depth {:.1}m | compactness {:.0} m^2 | support {:.0}% | rest defense {:.0}%\n",
                sample.timestamp_secs,
                team_label(sample.team, &metrics.coach_metrics),
                sample.width_m,
                sample.depth_m,
                sample.compactness_m2,
                sample.support_score * 100.0,
                sample.rest_defense_score * 100.0,
            ));
        }
    }

    if !metrics.coach_metrics.frame_line_samples.is_empty() {
        report.push_str("\nFrame line samples:\n");
        for sample in metrics.coach_metrics.frame_line_samples.iter().take(8) {
            report.push_str(&format!(
                "- {:.1}s | {} | B-M {:.1}m | M-F {:.1}m | between-lines {:.0}%\n",
                sample.timestamp_secs,
                team_label(sample.team, &metrics.coach_metrics),
                sample.back_to_mid_spacing_m,
                sample.mid_to_front_spacing_m,
                sample.between_lines_occupation_score * 100.0,
            ));
        }
    }

    if !metrics.coach_metrics.possession_contexts.is_empty() {
        report.push_str("\nBall context:\n");
        for ctx in metrics.coach_metrics.possession_contexts.iter().take(8) {
            report.push_str(&format!(
                "- {:.1}s | {} in possession | support {} | central receiver {} | spare {:.0}%\n",
                ctx.timestamp_secs,
                ctx.team_in_possession,
                ctx.support_near_ball,
                if ctx.central_receiver_available {
                    "yes"
                } else {
                    "no"
                },
                ctx.spare_player_score * 100.0,
            ));
        }
    }

    if !metrics.coach_metrics.structural_alerts.is_empty() {
        report.push_str("\nStructural alerts:\n");
        for alert in metrics.coach_metrics.structural_alerts.iter().take(6) {
            report.push_str(&format!(
                "- {} | {}: {}\n",
                team_label(alert.team, &metrics.coach_metrics), alert.title, alert.description
            ));
        }
    }

    // ── MATCH PLAN ────────────────────────────────────────────────────────────
    let ma = &metrics.match_analysis;

    report.push_str("\n── MATCH PLAN: POSSESSION ────────────────────\n\n");
    if ma.possession.teams.is_empty() {
        report.push_str("No possession samples recorded.\n");
    } else {
        report.push_str(&format!(
            "Total sampled: {:.1}s\n",
            ma.possession.total_sampled_secs
        ));
        for tp in &ma.possession.teams {
            report.push_str(&format!(
                "\n{} — {:.1}s ({:.1}%)\n",
                team_label(tp.team, &metrics.coach_metrics), tp.time_secs, tp.share_pct
            ));
            report.push_str("  Thirds:");
            for third in [
                crate::metrics::match_analysis::PitchThird::Defensive,
                crate::metrics::match_analysis::PitchThird::Middle,
                crate::metrics::match_analysis::PitchThird::Attacking,
            ] {
                let t = tp.third_time_secs.get(&third).copied().unwrap_or(0.0);
                report.push_str(&format!("  {} {:.1}s", third, t));
            }
            report.push('\n');
            report.push_str("  Lanes: ");
            for lane in [
                crate::metrics::match_analysis::PitchLane::Left,
                crate::metrics::match_analysis::PitchLane::Central,
                crate::metrics::match_analysis::PitchLane::Right,
            ] {
                let t = tp.lane_time_secs.get(&lane).copied().unwrap_or(0.0);
                report.push_str(&format!("  {} {:.1}s", lane, t));
            }
            report.push('\n');
            report.push_str(&format!(
                "  Switches: {} | Avg switch: {:.1}s | Longest same-side: {:.1}s\n",
                tp.switch_count, tp.avg_switch_time_secs, tp.longest_same_side_stretch_secs
            ));
        }
    }

    report.push_str("\n── MATCH PLAN: CROSSING ──────────────────────\n\n");
    if ma.crossing.events.is_empty() {
        report.push_str("No crosses detected.\n");
    } else {
        for team in [
            crate::metrics::coach::TeamId::TeamA,
            crate::metrics::coach::TeamId::TeamB,
        ] {
            let n = ma.crossing.by_team.get(&team).copied().unwrap_or(0);
            report.push_str(&format!("- {}: {} crosses\n", team_label(team, &metrics.coach_metrics), n));
        }
        report.push('\n');
        report.push_str("  Time  | Team       | Side  | Origin        | Atk/Def | Zones\n");
        report.push_str("  ──────|────────────|───────|───────────────|─────────|──────\n");
        for ev in ma.crossing.events.iter().take(20) {
            let mut zones: Vec<String> =
                ev.attacker_zones.iter().map(|z| z.to_string()).collect();
            zones.sort();
            report.push_str(&format!(
                "  {:>5.1}s| {:<10}| {:<6}| {:>5.1},{:>5.1}  |  {} vs {}  | {}\n",
                ev.timestamp_secs,
                team_label(ev.attacking_team, &metrics.coach_metrics),
                ev.side,
                ev.origin.x,
                ev.origin.y,
                ev.attackers_in_box,
                ev.defenders_in_box,
                zones.join(",")
            ));
        }
    }

    report.push_str("\n── MATCH PLAN: RUNNING ───────────────────────\n\n");
    if ma.running.players.is_empty() {
        report.push_str("No running data.\n");
    } else {
        for tr in &ma.running.teams {
            report.push_str(&format!(
                "- {} | {:.0} m total | HSR {:.1}s | {} sprints | {} players\n",
                team_label(tr.team, &metrics.coach_metrics),
                tr.total_distance_m,
                tr.high_speed_run_secs,
                tr.sprint_count,
                tr.players_counted
            ));
        }
        report.push('\n');
        report.push_str("  ID   | Team       | Dist(m) | HSR(s) | Sprints | Max(km/h)\n");
        report.push_str("  ─────|────────────|─────────|────────|─────────|──────────\n");
        for p in ma.running.players.iter().take(20) {
            let team = p
                .team
                .map(|t| team_label(t, &metrics.coach_metrics))
                .unwrap_or_else(|| "—".to_string());
            report.push_str(&format!(
                "  #{:<4}| {:<10}| {:<7.0} | {:<6.1} | {:<7} | {:<8.1}\n",
                p.track_id,
                team,
                p.total_distance_m,
                p.high_speed_run_secs,
                p.sprint_count,
                p.max_speed_kmh
            ));
        }
    }

    report.push_str("\n── MATCH PLAN: WEAKEST PLAYERS ───────────────\n\n");
    if ma.weakest_players.is_empty() {
        report.push_str("Not enough data to rank weakest players.\n");
    } else {
        report.push_str(
            "Composite: low activity (30%), low speed (20%), turnovers (25%), 1v1 losses (25%).\n\n",
        );
        report.push_str(
            "  Rank | ID   | Team       | Weak | Turnovers | Duel loss | Notes\n",
        );
        report.push_str(
            "  ─────|──────|────────────|──────|───────────|───────────|──────\n",
        );
        for (i, w) in ma.weakest_players.iter().take(10).enumerate() {
            let team = w
                .team
                .map(|t| team_label(t, &metrics.coach_metrics))
                .unwrap_or_else(|| "—".to_string());
            let notes = if w.notes.is_empty() {
                "".to_string()
            } else {
                w.notes.clone()
            };
            report.push_str(&format!(
                "  {:<4} | #{:<4}| {:<10}| {:<4.2} | {:<9.2} | {:<9.2} | {}\n",
                i + 1,
                w.track_id,
                team,
                w.weakness,
                w.turnover_factor,
                w.duel_loss_factor,
                notes
            ));
        }
    }

    report.push_str("\nNotes: Automatic pitch awareness is used when the software can confidently see the field shape and lines.\n");
    report.push_str("\n═══════════════════════════════════════════════\n");
    report
}
