// gui/analysis_panel.rs — Right-side panel: controls + analysis tabs

use super::app::{AnalysisTab, CoachApp};
use super::colors;
use crate::detection::COCO_SPORTS_BALL;
use crate::metrics::coach::{team_label, CoachMetrics, TeamId};
use crate::metrics::match_analysis::{
    CrossEvent, MatchAnalysis, PitchLane, PitchThird, WeakPlayerScore,
};
use eframe::egui;

pub fn show(app: &mut CoachApp, ui: &mut egui::Ui) {
    ui.heading(egui::RichText::new("Analysis").color(colors::ACCENT));
    ui.separator();

    // --- Settings section ---
    egui::CollapsingHeader::new("Settings")
        .default_open(true)
        .show(ui, |ui| {
            // Model info
            if let Some(status) = &app.library_status {
                ui.label(
                    egui::RichText::new(status)
                        .color(colors::ACCENT)
                        .small(),
                );
                ui.add_space(4.0);
            }

            ui.horizontal(|ui| {
                ui.label("Model:");
                if let Some(path) = &app.model_path {
                    ui.label(
                        egui::RichText::new(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        )
                        .color(colors::ACCENT)
                        .monospace(),
                    );
                } else {
                    ui.label(egui::RichText::new("None loaded").color(colors::TEXT_SECONDARY));
                }
            });

            // Video info
            if let Some(info) = &app.video_info {
                ui.horizontal(|ui| {
                    ui.label("Video:");
                    ui.label(format!(
                        "{}x{} @ {:.1} fps ({:.1}s)",
                        info.width, info.height, info.fps, info.duration_secs
                    ));
                });
            }

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Library").strong());
            if app.library.items().is_empty() {
                ui.label(
                    egui::RichText::new("No media stored yet.")
                        .color(colors::TEXT_SECONDARY)
                        .small(),
                );
            } else {
                let recent_items: Vec<_> = app.library.items().iter().take(6).cloned().collect();
                for item in recent_items {
                    let duration = item
                        .duration_secs
                        .map(|secs| format!("{secs:.1}s"))
                        .unwrap_or_else(|| "unknown".to_string());
                    ui.horizontal(|ui| {
                        if ui.small_button("Load").clicked() {
                            app.load_library_item(&item);
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{} | {} | {}",
                                item.kind,
                                item.title,
                                duration,
                            ))
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                        );
                    });
                }
            }

            ui.add_space(8.0);

            if let Some(mapper) = &app.mapper {
                ui.label(
                    egui::RichText::new(format!(
                        "Pitch mapping: {}",
                        mapper.calibration.mode
                    ))
                    .color(colors::ACCENT),
                );
            } else if app.auto_pitch_ready {
                ui.label(
                    egui::RichText::new("Pitch awareness: auto-detected from the current clip")
                        .color(colors::ACCENT)
                        .small(),
                );
            } else {
                ui.label(
                    egui::RichText::new(
                        "Pitch awareness: no automatic mapping yet, manual landmarks still available",
                    )
                    .color(colors::TEXT_SECONDARY)
                    .small(),
                );
            }

            if let Some(scene) = &app.scene_awareness {
                ui.label(
                        egui::RichText::new(format!(
                        "Pitch confidence {:.0}% | Tactical view {:.0}% | Field {:.0}% | Lines {:.0}% | Goal side {}",
                        scene.confidence * 100.0,
                        crate::pitch_awareness::tactical_view_score(scene) * 100.0,
                        scene.field_mask_ratio * 100.0,
                        (scene.line_ratio * 100.0).min(100.0),
                        scene.goal_side_hint,
                    ))
                    .color(colors::TEXT_SECONDARY)
                    .small(),
                );
            }

            if app.trim_suggestion.is_some() {
                ui.add_space(4.0);
                ui.checkbox(&mut app.use_trimmed_segments, "Auto-trim non-play shots");
                ui.checkbox(&mut app.playback_trimmed_only, "Playback only kept segments");
                let (segment_len, enabled_count, enabled_frames, enabled_duration):
                    (usize, usize, usize, f64) = {
                    let trim = app.trim_suggestion.as_ref().unwrap();
                    (
                        trim.segments.len(),
                        trim.segments.iter().filter(|segment| segment.enabled).count(),
                        trim.segments
                            .iter()
                            .filter(|segment| segment.enabled)
                            .map(|segment| segment.end_idx - segment.start_idx + 1)
                            .sum(),
                        trim.segments
                            .iter()
                            .filter(|segment| segment.enabled)
                            .map(|segment| (segment.end_secs - segment.start_secs).max(0.0))
                            .sum(),
                    )
                };

                if segment_len == 0 {
                    ui.label(
                        egui::RichText::new("No reliable play segments detected yet.")
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!(
                            "Trim keeps {} enabled segments, {:.1}s total, {} frames",
                            enabled_count,
                            enabled_duration,
                            enabled_frames,
                        ))
                        .color(colors::ACCENT)
                        .small(),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Drag segment edges on the bottom timeline to fine-tune in/out points.",
                        )
                        .color(colors::TEXT_SECONDARY)
                        .small(),
                    );

                    let segment_count = segment_len.min(6);
                    let mut jump_to_segment = None;
                    if let Some(trim) = &mut app.trim_suggestion {
                        for (idx, segment) in trim.segments.iter_mut().take(segment_count).enumerate() {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut segment.enabled, "");
                                if ui.button(format!("Go {}", idx + 1)).clicked() {
                                    jump_to_segment = Some(idx);
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1}s-{:.1}s ({:.0}% confidence)",
                                        segment.start_secs,
                                        segment.end_secs,
                                        segment.avg_confidence * 100.0,
                                    ))
                                    .color(if segment.enabled {
                                        colors::TEXT_PRIMARY
                                    } else {
                                        colors::TEXT_SECONDARY
                                    })
                                    .small(),
                                );
                            });
                        }
                    }

                    if let Some(idx) = jump_to_segment {
                        app.jump_to_segment(idx);
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Keep All").clicked() {
                            if let Some(trim) = &mut app.trim_suggestion {
                                for segment in &mut trim.segments {
                                    segment.enabled = true;
                                }
                            }
                        }
                        if ui.button("Drop All").clicked() {
                            if let Some(trim) = &mut app.trim_suggestion {
                                for segment in &mut trim.segments {
                                    segment.enabled = false;
                                }
                            }
                        }
                        if ui.button("Recompute Trim").clicked() {
                            app.trim_suggestion = Some(crate::pitch_awareness::detect_relevant_segments(
                                &app.frames,
                                10,
                                0.42,
                                1.5,
                                20,
                            ));
                        }
                    });

                    if segment_len > segment_count {
                        ui.label(
                            egui::RichText::new(format!(
                                "Showing first {} of {} segments",
                                segment_count,
                                segment_len
                            ))
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                        );
                    }
                }
            }

            ui.add_space(4.0);

            // Detection threshold
            ui.add(
                egui::Slider::new(&mut app.conf_threshold, 0.1..=0.9).text("Confidence threshold"),
            );

            // Overlay toggles
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.show_bboxes, "Boxes");
                ui.checkbox(&mut app.show_ids, "IDs");
                ui.checkbox(&mut app.show_trails, "Trails");
            });

            // Homography corners
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.editing_corners, "Edit pitch landmarks");
                if app.editing_corners {
                    ui.label(
                        egui::RichText::new(format!(
                            "Click to set point {} ({})",
                            app.selected_reference_point + 1,
                            app.selected_reference_point().label()
                        ))
                        .color(egui::Color32::YELLOW)
                        .small(),
                    );
                }
            });

            if app.editing_corners {
                ui.label(
                    egui::RichText::new(
                        "Pick any 4 landmarks visible in the clip if the auto pitch awareness is not good enough.",
                    )
                    .color(colors::TEXT_SECONDARY)
                    .small(),
                );
                ui.add_space(6.0);

                for i in 0..app.homography_calibration.reference_points.len() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Point {}", i + 1));

                        egui::ComboBox::from_id_salt(("pitch_reference", i))
                            .selected_text(app.homography_calibration.reference_points[i].label())
                            .show_ui(ui, |ui| {
                                for point in crate::pitch_mapping::PitchReferencePoint::all() {
                                    ui.selectable_value(
                                        &mut app.homography_calibration.reference_points[i],
                                        *point,
                                        point.label(),
                                    );
                                }
                            });

                        if ui
                            .selectable_label(app.selected_reference_point == i, "Select")
                            .clicked()
                        {
                            app.selected_reference_point = i;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        match app.homography_calibration.image_points[i] {
                            Some(pt) => {
                                ui.label(format!("image: ({:.0}, {:.0})", pt.x, pt.y));
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new("image: not set")
                                        .color(colors::TEXT_SECONDARY),
                                );
                            }
                        }

                        let pitch_pt = app.homography_calibration.reference_points[i].pitch_point();
                        ui.label(
                            egui::RichText::new(format!(
                                "pitch: ({:.1}m, {:.1}m)",
                                pitch_pt.x, pitch_pt.y
                            ))
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                        );
                    });
                }

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} / 4 landmarks set",
                        app.homography_calibration.completion_count()
                    ))
                    .color(colors::ACCENT)
                    .small(),
                );

                if ui.button("Clear Calibration").clicked() {
                    app.homography_calibration.image_points = [None; 4];
                    app.homography_calibration.explicit_pitch_points = None;
                    app.homography_calibration.mode = crate::pitch_mapping::CalibrationMode::Manual;
                    app.selected_reference_point = 0;
                }
            }
        });

    ui.separator();

    // --- Tab bar ---
    ui.horizontal(|ui| {
        let tabs = [
            (AnalysisTab::MatchPlan, "Match Plan"),
            (AnalysisTab::FourView, "4-View"),
            (AnalysisTab::Coach, "Coach"),
            (AnalysisTab::Insights, "Insights"),
            (AnalysisTab::Tracking, "Tracking"),
            (AnalysisTab::Heatmaps, "Heatmaps"),
            (AnalysisTab::PitchView, "Pitch View"),
            (AnalysisTab::Report, "Report"),
        ];

        for (tab, label) in &tabs {
            let selected = app.active_tab == *tab;
            let text = if selected {
                egui::RichText::new(*label).strong().color(colors::ACCENT)
            } else {
                egui::RichText::new(*label).color(colors::TEXT_SECONDARY)
            };
            if ui.selectable_label(selected, text).clicked() {
                app.active_tab = *tab;
            }
        }
    });

    ui.separator();

    // --- Tab content ---
    egui::ScrollArea::vertical().show(ui, |ui| match app.active_tab {
        AnalysisTab::MatchPlan => show_match_plan_tab(app, ui),
        AnalysisTab::FourView => show_four_view_tab(app, ui),
        AnalysisTab::Coach => show_coach_tab(app, ui),
        AnalysisTab::Insights => show_insights_tab(app, ui),
        AnalysisTab::Tracking => show_tracking_tab(app, ui),
        AnalysisTab::Heatmaps => show_heatmaps_tab(app, ui),
        AnalysisTab::PitchView => show_pitch_view_tab(app, ui),
        AnalysisTab::Report => show_report_tab(app, ui),
    });
}

fn show_coach_tab(app: &CoachApp, ui: &mut egui::Ui) {
    let Some(metrics) = &app.metrics else {
        ui.label(
            egui::RichText::new("Run analysis to see coach-oriented structural review.")
                .color(colors::TEXT_SECONDARY),
        );
        return;
    };

    ui.label(egui::RichText::new("Phase Review").strong());
    if let Some(phase) = &metrics.coach_metrics.dominant_phase {
        ui.label(format!("Dominant phase: {}", phase));
    }
    ui.add_space(6.0);

    ui.label(egui::RichText::new("Team Shape").strong());
    for shape in metrics.coach_metrics.team_shapes.values() {
        ui.label(format!(
            "{} | width {:.1}m | depth {:.1}m | line height {:.1}m | compactness {:.0} m²",
            team_label(shape.team, &metrics.coach_metrics), shape.width_m, shape.depth_m, shape.line_height_m, shape.compactness_m2
        ));
    }

    if !metrics.coach_metrics.team_colors.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Team Color Signatures").strong());
        for (team, color) in &metrics.coach_metrics.team_colors {
            ui.label(format!(
                "{} | rgb {:.2}/{:.2}/{:.2} | sat {:.2}",
                team_label(*team, &metrics.coach_metrics), color.r, color.g, color.b, color.saturation
            ));
        }
    }

    if !metrics.coach_metrics.team_lines.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Line / Unit Analysis").strong());
        for lines in metrics.coach_metrics.team_lines.values() {
            ui.label(format!(
                "{} | back {:.1}m | mid {:.1}m | front {:.1}m | B-M {:.1}m | M-F {:.1}m | between-lines {:.0}%",
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
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Frame Shape Samples").strong());
        for sample in metrics.coach_metrics.frame_team_shapes.iter().take(8) {
            ui.label(
                egui::RichText::new(format!(
                    "{:.1}s | {} | width {:.1}m | depth {:.1}m | compactness {:.0} m² | support {:.0}% | rest defense {:.0}%",
                    sample.timestamp_secs,
                    team_label(sample.team, &metrics.coach_metrics),
                    sample.width_m,
                    sample.depth_m,
                    sample.compactness_m2,
                    sample.support_score * 100.0,
                    sample.rest_defense_score * 100.0,
                ))
                .color(colors::TEXT_SECONDARY),
            );
        }
    }

    if !metrics.coach_metrics.frame_line_samples.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Frame Line Samples").strong());
        for sample in metrics.coach_metrics.frame_line_samples.iter().take(8) {
            ui.label(
                egui::RichText::new(format!(
                    "{:.1}s | {} | B-M {:.1}m | M-F {:.1}m | between-lines {:.0}%",
                    sample.timestamp_secs,
                    team_label(sample.team, &metrics.coach_metrics),
                    sample.back_to_mid_spacing_m,
                    sample.mid_to_front_spacing_m,
                    sample.between_lines_occupation_score * 100.0,
                ))
                .color(colors::TEXT_SECONDARY),
            );
        }
    }

    if !metrics.coach_metrics.possession_contexts.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Ball Context").strong());
        for ctx in metrics.coach_metrics.possession_contexts.iter().take(8) {
            ui.label(
                egui::RichText::new(format!(
                    "{:.1}s | {} in possession | support {} | central receiver {} | spare {:.0}%",
                    ctx.timestamp_secs,
                    ctx.team_in_possession,
                    ctx.support_near_ball,
                    if ctx.central_receiver_available {
                        "yes"
                    } else {
                        "no"
                    },
                    ctx.spare_player_score * 100.0,
                ))
                .color(colors::TEXT_SECONDARY),
            );
        }
    }

    ui.add_space(10.0);
    ui.label(egui::RichText::new("Structural Alerts").strong());
    if metrics.coach_metrics.structural_alerts.is_empty() {
        ui.label(
            egui::RichText::new("No major structural alerts detected in this clip.")
                .color(colors::TEXT_SECONDARY),
        );
    } else {
        for alert in &metrics.coach_metrics.structural_alerts {
            ui.label(
                egui::RichText::new(format!(
                    "{} | {}",
                    team_label(alert.team, &metrics.coach_metrics),
                    alert.title
                ))
                .strong(),
            );
            ui.label(egui::RichText::new(&alert.description).color(colors::TEXT_SECONDARY));
            ui.add_space(6.0);
        }
    }

    ui.add_space(10.0);
    ui.label(egui::RichText::new("Phase Events").strong());
    for event in metrics.coach_metrics.phase_events.iter().take(12) {
        ui.label(
            egui::RichText::new(format!(
                "{:.1}s | {} | {}",
                event.timestamp_secs, event.label, event.area
            ))
            .color(colors::TEXT_SECONDARY),
        );
    }
}

fn show_insights_tab(app: &CoachApp, ui: &mut egui::Ui) {
    if app.insights.is_empty() {
        ui.label(
            egui::RichText::new("Run analysis to generate tactical insights.")
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }

    for insight in &app.insights {
        let importance_color = match insight.importance {
            crate::tactical_insights::Importance::High => egui::Color32::from_rgb(255, 80, 80),
            crate::tactical_insights::Importance::Medium => egui::Color32::from_rgb(255, 180, 50),
            crate::tactical_insights::Importance::Low => colors::TEXT_SECONDARY,
        };

        let importance_label = match insight.importance {
            crate::tactical_insights::Importance::High => "HIGH",
            crate::tactical_insights::Importance::Medium => "MED",
            crate::tactical_insights::Importance::Low => "LOW",
        };

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("[{}]", importance_label))
                    .color(importance_color)
                    .monospace()
                    .small(),
            );
            ui.label(
                egui::RichText::new(format!("[{}]", insight.category))
                    .color(colors::TEXT_SECONDARY)
                    .monospace()
                    .small(),
            );
        });
        ui.label(egui::RichText::new(&insight.title).strong());
        ui.label(egui::RichText::new(&insight.description).color(colors::TEXT_SECONDARY));
        ui.separator();
    }
}

fn show_tracking_tab(app: &mut CoachApp, ui: &mut egui::Ui) {
    if let Some(tracking) = &app.tracking {
        ui.label(format!("Total unique tracks: {}", tracking.total_tracks));
        ui.add_space(8.0);

        // Show current frame tracks
        let frame_idx = match app.current_frame() {
            Some(frame) => frame.index,
            None => return,
        };
        if let Some(ft) = tracking
            .frame_tracks
            .iter()
            .find(|ft| ft.frame_index == frame_idx)
        {
            ui.label(
                egui::RichText::new(format!(
                    "Frame {} — {} objects:",
                    frame_idx,
                    ft.tracks.len()
                ))
                .strong(),
            );
            ui.add_space(4.0);

            for track in &ft.tracks {
                let selected = app.selected_player == Some(track.track_id);
                let text = format!(
                    "#{} {} — {:.0}% vel:({:.1},{:.1})",
                    track.track_id,
                    track.class_name,
                    track.confidence * 100.0,
                    track.velocity.0,
                    track.velocity.1,
                );
                if ui.selectable_label(selected, text).clicked() {
                    app.selected_player = Some(track.track_id);
                }
            }
        }

        // Player metrics table
        if let Some(metrics) = &app.metrics {
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Player Stats").strong());

            egui::Grid::new("player_stats_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("ID").strong());
                    ui.label(egui::RichText::new("Dist (m)").strong());
                    ui.label(egui::RichText::new("Avg km/h").strong());
                    ui.label(egui::RichText::new("Max km/h").strong());
                    ui.end_row();

                    let mut players: Vec<_> = metrics.player_metrics.values().collect();
                    players.retain(|player| player.total_distance_m.is_finite());
                    players.sort_by(|a, b| b.total_distance_m.total_cmp(&a.total_distance_m));

                    for p in players.iter().take(15) {
                        ui.label(format!("#{}", p.track_id));
                        ui.label(format!("{:.1}", p.total_distance_m));
                        ui.label(format!("{:.1}", p.avg_speed_kmh));
                        ui.label(format!("{:.1}", p.max_speed_kmh));
                        ui.end_row();
                    }
                });
        }
    } else {
        ui.label(
            egui::RichText::new("Run analysis to see tracking data.").color(colors::TEXT_SECONDARY),
        );
    }
}

fn show_heatmaps_tab(app: &CoachApp, ui: &mut egui::Ui) {
    if let Some(metrics) = &app.metrics {
        ui.label(egui::RichText::new("Team Heatmap (all players)").strong());
        ui.add_space(8.0);

        super::heatmap_view::draw_heatmap(ui, &metrics.heatmap_data, None);

        ui.add_space(16.0);

        // Per-player heatmaps
        if let Some(player_id) = app.selected_player {
            ui.label(egui::RichText::new(format!("Player #{} Heatmap", player_id)).strong());
            ui.add_space(8.0);
            super::heatmap_view::draw_heatmap(ui, &metrics.heatmap_data, Some(player_id));
        } else {
            ui.label(
                egui::RichText::new(
                    "Select a player in the Tracking tab to see individual heatmap.",
                )
                .color(colors::TEXT_SECONDARY),
            );
        }
    } else {
        ui.label(
            egui::RichText::new("Run analysis to generate heatmaps.").color(colors::TEXT_SECONDARY),
        );
    }
}

fn show_report_tab(app: &CoachApp, ui: &mut egui::Ui) {
    if let Some(metrics) = &app.metrics {
        let video_path = app
            .video_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let report = crate::export::generate_text_report(&video_path, metrics, &app.insights);

        ui.add(
            egui::TextEdit::multiline(&mut report.as_str())
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    } else {
        ui.label(
            egui::RichText::new("Run analysis to generate the tactical report.")
                .color(colors::TEXT_SECONDARY),
        );
    }
}

fn show_pitch_view_tab(app: &CoachApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Top-Down Pitch View").strong());
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Live player positions mapped onto a 2D pitch diagram.")
            .color(colors::TEXT_SECONDARY)
            .small(),
    );
    ui.add_space(8.0);

    // Get current frame tracks
    let frame_tracks = app.tracking.as_ref().and_then(|t| {
        let idx = app.current_frame()?.index;
        t.frame_tracks.iter().find(|ft| ft.frame_index == idx)
    });

    let mapper = app.mapper.as_ref();

    if frame_tracks.is_none() && mapper.is_none() {
        ui.label(
            egui::RichText::new("Run analysis to see the pitch overlay.")
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }

    let coach = app.metrics.as_ref().map(|m| &m.coach_metrics);
    super::pitch_overlay::draw_pitch_overlay(ui, frame_tracks, mapper, coach);

    // Legend
    ui.add_space(12.0);
    let (label_a, label_b, color_a, color_b) = match app.metrics.as_ref() {
        Some(m) => (
            team_label(TeamId::TeamA, &m.coach_metrics),
            team_label(TeamId::TeamB, &m.coach_metrics),
            team_color(TeamId::TeamA, &m.coach_metrics),
            team_color(TeamId::TeamB, &m.coach_metrics),
        ),
        None => (
            "Team A".to_string(),
            "Team B".to_string(),
            colors::PLAYER_TEAM_A,
            colors::PLAYER_TEAM_B,
        ),
    };
    ui.horizontal(|ui| {
        let dot_size = 8.0;
        let (rect_a, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_a.center(), dot_size / 2.0, color_a);
        ui.label(&label_a);
        ui.add_space(8.0);
        let (rect_b, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_b.center(), dot_size / 2.0, color_b);
        ui.label(&label_b);
        ui.add_space(8.0);
        let (rect_ball, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_ball.center(), dot_size / 2.0, colors::BALL_COLOR);
        ui.label("Ball");
    });
}

// ==================== MATCH PLAN TAB ====================

fn show_match_plan_tab(app: &CoachApp, ui: &mut egui::Ui) {
    let Some(metrics) = &app.metrics else {
        ui.label(
            egui::RichText::new(
                "Run analysis to see the Match Plan: possession, crossing, running, and weakest players.",
            )
            .color(colors::TEXT_SECONDARY),
        );
        return;
    };
    let ma = &metrics.match_analysis;
    let coach = &metrics.coach_metrics;

    ui.label(
        egui::RichText::new("Match Plan")
            .heading()
            .color(colors::ACCENT),
    );
    ui.label(
        egui::RichText::new(
            "Coach-focused view: who owns the ball and where, how they cross, \
             who runs the most, and who is the weakest link.",
        )
        .color(colors::TEXT_SECONDARY)
        .small(),
    );
    ui.add_space(10.0);

    show_possession_section(ma, coach, ui);
    ui.add_space(14.0);
    show_crossing_section(ma, coach, ui);
    ui.add_space(14.0);
    show_running_section(ma, coach, ui);
    ui.add_space(14.0);
    show_weakest_section(ma, coach, ui);
}

fn show_possession_section(ma: &MatchAnalysis, coach: &CoachMetrics, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(
        egui::RichText::new("Possession — time, location & switches").strong(),
    )
    .default_open(true)
    .show(ui, |ui| {
        let poss = &ma.possession;
        if poss.teams.is_empty() {
            ui.label(
                egui::RichText::new("No possession samples recorded.")
                    .color(colors::TEXT_SECONDARY),
            );
            return;
        }

        // Possession share bar
        ui.label(format!(
            "Total sampled: {:.1}s",
            poss.total_sampled_secs
        ));
        ui.add_space(4.0);

        let team_a = poss.teams.iter().find(|t| t.team == TeamId::TeamA);
        let team_b = poss.teams.iter().find(|t| t.team == TeamId::TeamB);
        let share_a = team_a.map(|t| t.share_pct).unwrap_or(0.0);
        let share_b = team_b.map(|t| t.share_pct).unwrap_or(0.0);

        let bar_w = ui.available_width().min(520.0);
        let bar_h = 18.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, bar_h), egui::Sense::hover());
        let painter = ui.painter();
        let a_frac = (share_a / 100.0).clamp(0.0, 1.0) as f32;
        let split_x = rect.left() + rect.width() * a_frac;
        painter.rect_filled(
            egui::Rect::from_min_max(rect.left_top(), egui::pos2(split_x, rect.bottom())),
            2.0,
            team_color(TeamId::TeamA, coach),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(split_x, rect.top()), rect.right_bottom()),
            2.0,
            team_color(TeamId::TeamB, coach),
        );
        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{} {:.0}%", team_label(TeamId::TeamA, coach), share_a),
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
        painter.text(
            rect.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{:.0}% {}", share_b, team_label(TeamId::TeamB, coach)),
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );

        ui.add_space(8.0);

        // Per-team breakdown
        for tp in &poss.teams {
            ui.label(
                egui::RichText::new(format!(
                    "{} — {:.1}s ({:.1}%)",
                    team_label(tp.team, coach),
                    tp.time_secs,
                    tp.share_pct
                ))
                .strong()
                .color(team_color(tp.team, coach)),
            );

            // Thirds
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Thirds:").color(colors::TEXT_SECONDARY));
                for third in [PitchThird::Defensive, PitchThird::Middle, PitchThird::Attacking] {
                    let t = tp.third_time_secs.get(&third).copied().unwrap_or(0.0);
                    ui.label(format!("{} {:.1}s", third, t));
                }
            });
            // Lanes
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Lanes:").color(colors::TEXT_SECONDARY));
                for lane in [PitchLane::Left, PitchLane::Central, PitchLane::Right] {
                    let t = tp.lane_time_secs.get(&lane).copied().unwrap_or(0.0);
                    ui.label(format!("{} {:.1}s", lane, t));
                }
            });
            // Switches
            ui.label(format!(
                "Switches: {}  |  Avg switch time: {:.1}s  |  Longest same-side stretch: {:.1}s",
                tp.switch_count, tp.avg_switch_time_secs, tp.longest_same_side_stretch_secs
            ));
            ui.add_space(6.0);
        }
    });
}

fn show_crossing_section(ma: &MatchAnalysis, coach: &CoachMetrics, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(
        egui::RichText::new("Crossing — events, origin & box load").strong(),
    )
    .default_open(true)
    .show(ui, |ui| {
        let cx = &ma.crossing;
        if cx.events.is_empty() {
            ui.label(
                egui::RichText::new("No crosses detected in this clip.")
                    .color(colors::TEXT_SECONDARY),
            );
            return;
        }

        ui.horizontal(|ui| {
            for team in [TeamId::TeamA, TeamId::TeamB] {
                let n = cx.by_team.get(&team).copied().unwrap_or(0);
                ui.label(
                    egui::RichText::new(format!("{}: {} crosses", team_label(team, coach), n))
                        .strong()
                        .color(team_color(team, coach)),
                );
            }
        });
        ui.add_space(6.0);

        // Table-like rows
        egui::Grid::new("cross_events_grid")
            .striped(true)
            .num_columns(6)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Time").strong());
                ui.label(egui::RichText::new("Team").strong());
                ui.label(egui::RichText::new("Side").strong());
                ui.label(egui::RichText::new("Origin (x, y)").strong());
                ui.label(egui::RichText::new("Atk / Def in box").strong());
                ui.label(egui::RichText::new("Attacker zones").strong());
                ui.end_row();

                for ev in cx.events.iter().take(40) {
                    ui.label(format!("{:.1}s", ev.timestamp_secs));
                    ui.label(
                        egui::RichText::new(team_label(ev.attacking_team, coach))
                            .color(team_color(ev.attacking_team, coach)),
                    );
                    ui.label(ev.side.to_string());
                    ui.label(format!("{:.1}, {:.1}", ev.origin.x, ev.origin.y));
                    ui.label(format!(
                        "{} vs {}",
                        ev.attackers_in_box, ev.defenders_in_box
                    ));
                    ui.label(zones_summary(ev));
                    ui.end_row();
                }
            });

        if cx.events.len() > 40 {
            ui.label(
                egui::RichText::new(format!("… {} more", cx.events.len() - 40))
                    .color(colors::TEXT_SECONDARY),
            );
        }
    });
}

fn zones_summary(ev: &CrossEvent) -> String {
    if ev.attacker_zones.is_empty() {
        return "—".to_string();
    }
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for z in &ev.attacker_zones {
        *counts.entry(z.to_string()).or_insert(0) += 1;
    }
    let mut parts: Vec<String> = counts
        .into_iter()
        .map(|(k, v)| if v > 1 { format!("{}×{}", v, k) } else { k })
        .collect();
    parts.sort();
    parts.join(", ")
}

fn show_running_section(ma: &MatchAnalysis, coach: &CoachMetrics, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(
        egui::RichText::new("Running — distance, high-speed & sprints").strong(),
    )
    .default_open(true)
    .show(ui, |ui| {
        let r = &ma.running;
        if r.players.is_empty() {
            ui.label(
                egui::RichText::new("No running data available.")
                    .color(colors::TEXT_SECONDARY),
            );
            return;
        }

        // Team totals
        for tr in &r.teams {
            ui.label(
                egui::RichText::new(format!(
                    "{} — {:.0} m total, {:.1}s HSR, {} sprints ({} players)",
                    team_label(tr.team, coach),
                    tr.total_distance_m,
                    tr.high_speed_run_secs,
                    tr.sprint_count,
                    tr.players_counted
                ))
                .strong()
                .color(team_color(tr.team, coach)),
            );
        }
        ui.add_space(6.0);

        egui::Grid::new("running_grid")
            .striped(true)
            .num_columns(6)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Player").strong());
                ui.label(egui::RichText::new("Team").strong());
                ui.label(egui::RichText::new("Distance (m)").strong());
                ui.label(egui::RichText::new("HSR (s)").strong());
                ui.label(egui::RichText::new("Sprints").strong());
                ui.label(egui::RichText::new("Max (km/h)").strong());
                ui.end_row();

                for p in r.players.iter().take(30) {
                    ui.label(format!("#{}", p.track_id));
                    match p.team {
                        Some(t) => ui.label(
                            egui::RichText::new(team_label(t, coach)).color(team_color(t, coach)),
                        ),
                        None => ui.label(
                            egui::RichText::new("—").color(colors::TEXT_SECONDARY),
                        ),
                    };
                    ui.label(format!("{:.0}", p.total_distance_m));
                    ui.label(format!("{:.1}", p.high_speed_run_secs));
                    ui.label(format!("{}", p.sprint_count));
                    ui.label(format!("{:.1}", p.max_speed_kmh));
                    ui.end_row();
                }
            });
    });
}

fn show_weakest_section(ma: &MatchAnalysis, coach: &CoachMetrics, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(
        egui::RichText::new("Weakest players — target these").strong(),
    )
    .default_open(true)
    .show(ui, |ui| {
        if ma.weakest_players.is_empty() {
            ui.label(
                egui::RichText::new("Not enough data to rank weakest players.")
                    .color(colors::TEXT_SECONDARY),
            );
            return;
        }

        ui.label(
            egui::RichText::new(
                "Composite: low activity (30%), low speed (20%), turnovers (25%), 1v1 losses (25%). \
                 Normalized within team. Higher = weaker.",
            )
            .color(colors::TEXT_SECONDARY)
            .small(),
        );
        ui.add_space(4.0);

        egui::Grid::new("weakest_grid")
            .striped(true)
            .num_columns(7)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Rank").strong());
                ui.label(egui::RichText::new("Player").strong());
                ui.label(egui::RichText::new("Team").strong());
                ui.label(egui::RichText::new("Weakness").strong());
                ui.label(egui::RichText::new("Turnovers").strong());
                ui.label(egui::RichText::new("Duel loss").strong());
                ui.label(egui::RichText::new("Notes").strong());
                ui.end_row();

                for (i, w) in ma.weakest_players.iter().take(10).enumerate() {
                    ui.label(format!("{}", i + 1));
                    ui.label(format!("#{}", w.track_id));
                    match w.team {
                        Some(t) => ui.label(
                            egui::RichText::new(team_label(t, coach)).color(team_color(t, coach)),
                        ),
                        None => ui.label(
                            egui::RichText::new("—").color(colors::TEXT_SECONDARY),
                        ),
                    };
                    // Weakness bar
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(80.0, 12.0), egui::Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 2.0, egui::Color32::from_gray(50));
                    let frac = w.weakness.clamp(0.0, 1.0) as f32;
                    let fill = egui::Rect::from_min_size(
                        rect.left_top(),
                        egui::vec2(rect.width() * frac, rect.height()),
                    );
                    let color = weakness_color(w.weakness);
                    painter.rect_filled(fill, 2.0, color);
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{:.2}", w.weakness),
                        egui::FontId::monospace(10.0),
                        egui::Color32::WHITE,
                    );
                    ui.label(format!("{:.2}", w.turnover_factor));
                    ui.label(format!("{:.2}", w.duel_loss_factor));
                    ui.label(
                        egui::RichText::new(notes_summary(w))
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                    );
                    ui.end_row();
                }
            });
    });
}

fn notes_summary(w: &WeakPlayerScore) -> String {
    if !w.notes.is_empty() {
        return w.notes.clone();
    }
    let mut parts = Vec::new();
    if w.low_activity_factor > 0.6 {
        parts.push("low activity");
    }
    if w.low_speed_factor > 0.6 {
        parts.push("slow");
    }
    if w.turnover_factor > 0.5 {
        parts.push("gives ball away");
    }
    if w.duel_loss_factor > 0.5 {
        parts.push("loses 1v1");
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

fn weakness_color(w: f64) -> egui::Color32 {
    let w = w.clamp(0.0, 1.0) as f32;
    // green -> amber -> red
    if w < 0.5 {
        let t = w / 0.5;
        egui::Color32::from_rgb(
            (0.0 + t * 255.0) as u8,
            (180.0 - t * 40.0) as u8,
            (120.0 - t * 120.0) as u8,
        )
    } else {
        let t = (w - 0.5) / 0.5;
        egui::Color32::from_rgb(255, ((140.0) * (1.0 - t)) as u8, 0)
    }
}

fn team_color(team: TeamId, coach: &CoachMetrics) -> egui::Color32 {
    if let Some((r, g, b)) = crate::metrics::coach::team_display_rgb(team, coach) {
        return egui::Color32::from_rgb(r, g, b);
    }
    match team {
        TeamId::TeamA => colors::PLAYER_TEAM_A,
        TeamId::TeamB => colors::PLAYER_TEAM_B,
    }
}

// ==================== 4-VIEW TAB ====================

fn show_four_view_tab(app: &CoachApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("4-View — synced panes")
            .heading()
            .color(colors::ACCENT),
    );

    let ts_text = app
        .current_frame()
        .map(|f| format!("t = {:.2}s  |  frame {}", f.timestamp_secs, f.index + 1))
        .unwrap_or_else(|| "no frame loaded".to_string());
    ui.label(
        egui::RichText::new(format!(
            "Top-left: full tactical view.  Top-right: ball zoom.  \
             Bottom-left: weakest-player zoom.  Bottom-right: top-down pitch.   ({})",
            ts_text
        ))
        .color(colors::TEXT_SECONDARY)
        .small(),
    );
    ui.add_space(8.0);

    let avail = ui.available_size();
    let cell_w = (avail.x - 8.0) / 2.0;
    let cell_h = ((avail.y - 8.0) / 2.0).max(160.0);
    let cell_size = egui::vec2(cell_w, cell_h);

    let texture = app.frame_texture.as_ref();
    let tex_size = texture.map(|t| t.size_vec2()).unwrap_or(egui::vec2(1.0, 1.0));

    // Find the current frame's tracks
    let frame_idx = app.current_frame().map(|f| f.index);
    let frame_tracks = app.tracking.as_ref().and_then(|t| {
        let idx = frame_idx?;
        t.frame_tracks.iter().find(|ft| ft.frame_index == idx)
    });

    // Find ball bbox in current frame
    let ball_bbox = frame_tracks.and_then(|ft| {
        ft.tracks
            .iter()
            .find(|t| t.class_id == COCO_SPORTS_BALL)
            .map(|t| t.bbox)
    });

    // Find weakest player and their bbox
    let weakest = app
        .metrics
        .as_ref()
        .and_then(|m| m.match_analysis.weakest_players.first());
    let weakest_id = weakest.map(|w| w.track_id);
    let weakest_bbox = frame_tracks.and_then(|ft| {
        let id = weakest_id?;
        ft.tracks
            .iter()
            .find(|t| t.track_id == id)
            .map(|t| t.bbox)
    });

    let coach = app.metrics.as_ref().map(|m| &m.coach_metrics);

    // Sub-labels
    let ball_label = match ball_bbox {
        Some(b) => format!(
            "Ball zoom  ({:.0}, {:.0} px)",
            (b.x1 + b.x2) * 0.5,
            (b.y1 + b.y2) * 0.5
        ),
        None => "Ball zoom  (not visible)".to_string(),
    };

    let weakest_label = match weakest {
        Some(w) => {
            let team_txt = match (w.team, coach) {
                (Some(t), Some(c)) => format!("  [{}]", team_label(t, c)),
                _ => String::new(),
            };
            format!(
                "Weakest player #{}{}  —  weakness {:.2}",
                w.track_id, team_txt, w.weakness
            )
        }
        None => "Weakest player (n/a)".to_string(),
    };

    // --- Row 1 ---
    ui.horizontal(|ui| {
        draw_frame_cell(
            ui,
            cell_size,
            "Full view",
            texture,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            frame_tracks,
            tex_size,
            coach,
            Highlight {
                ball: ball_bbox.is_some(),
                weakest_id,
            },
        );
        ui.add_space(8.0);
        let uv = ball_bbox
            .map(|b| bbox_to_uv(b, tex_size, 4.0))
            .unwrap_or_else(|| egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
        draw_frame_cell(
            ui,
            cell_size,
            &ball_label,
            texture,
            uv,
            frame_tracks,
            tex_size,
            coach,
            Highlight {
                ball: true,
                weakest_id: None,
            },
        );
    });

    ui.add_space(8.0);

    // --- Row 2 ---
    ui.horizontal(|ui| {
        let uv = weakest_bbox
            .map(|b| bbox_to_uv(b, tex_size, 3.0))
            .unwrap_or_else(|| egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
        draw_frame_cell(
            ui,
            cell_size,
            &weakest_label,
            texture,
            uv,
            frame_tracks,
            tex_size,
            coach,
            Highlight {
                ball: false,
                weakest_id,
            },
        );
        ui.add_space(8.0);

        // Pitch map cell
        ui.allocate_ui_with_layout(
            cell_size,
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(
                    egui::RichText::new("Top-down pitch")
                        .color(colors::TEXT_SECONDARY)
                        .small(),
                );
                super::pitch_overlay::draw_pitch_overlay(
                    ui,
                    frame_tracks,
                    app.mapper.as_ref(),
                    coach,
                );
            },
        );
    });
}

#[derive(Copy, Clone, Default)]
struct Highlight {
    ball: bool,
    weakest_id: Option<u32>,
}

fn bbox_to_uv(
    bbox: crate::detection::BBox,
    tex_size: egui::Vec2,
    pad_factor: f32,
) -> egui::Rect {
    let cx = (bbox.x1 + bbox.x2) * 0.5;
    let cy = (bbox.y1 + bbox.y2) * 0.5;
    let w = (bbox.x2 - bbox.x1).abs().max(10.0) * pad_factor;
    let h = (bbox.y2 - bbox.y1).abs().max(10.0) * pad_factor;
    let half_w = w * 0.5;
    let half_h = h * 0.5;
    let tw = tex_size.x.max(1.0);
    let th = tex_size.y.max(1.0);
    let x1 = ((cx - half_w) / tw).clamp(0.0, 1.0);
    let y1 = ((cy - half_h) / th).clamp(0.0, 1.0);
    let x2 = ((cx + half_w) / tw).clamp(0.0, 1.0);
    let y2 = ((cy + half_h) / th).clamp(0.0, 1.0);
    egui::Rect::from_min_max(egui::pos2(x1, y1), egui::pos2(x2, y2))
}

#[allow(clippy::too_many_arguments)]
fn draw_frame_cell(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    uv: egui::Rect,
    frame_tracks: Option<&crate::tracker::FrameTracks>,
    tex_size: egui::Vec2,
    coach: Option<&CoachMetrics>,
    highlight: Highlight,
) {
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.label(
            egui::RichText::new(label)
                .color(colors::TEXT_SECONDARY)
                .small(),
        );
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(size.x, (size.y - 18.0).max(60.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(20));
        if let Some(tex) = texture {
            painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No frame",
                egui::FontId::proportional(12.0),
                colors::TEXT_SECONDARY,
            );
        }

        // Draw per-track overlays projected through the current UV
        if let Some(ft) = frame_tracks {
            let team_color_for = |team: TeamId| -> egui::Color32 {
                if let Some(c) = coach {
                    if let Some((r, g, b)) = crate::metrics::coach::team_display_rgb(team, c) {
                        return egui::Color32::from_rgb(r, g, b);
                    }
                }
                match team {
                    TeamId::TeamA => colors::PLAYER_TEAM_A,
                    TeamId::TeamB => colors::PLAYER_TEAM_B,
                }
            };

            for track in &ft.tracks {
                // Project texture-pixel coords through UV window into cell rect.
                let proj = project_bbox(track.bbox, tex_size, uv, rect);
                let Some(pr) = proj else {
                    continue;
                };
                if pr.width() < 2.0 || pr.height() < 2.0 {
                    continue;
                }

                if track.class_id == COCO_SPORTS_BALL {
                    let color = colors::BALL_COLOR;
                    painter.rect_stroke(
                        pr,
                        1.0,
                        egui::Stroke::new(1.5, color),
                        egui::StrokeKind::Outside,
                    );
                    if highlight.ball {
                        // reticle
                        painter.circle_stroke(
                            pr.center(),
                            (pr.width().max(pr.height()) * 0.8).max(10.0),
                            egui::Stroke::new(1.0, color.gamma_multiply(0.6)),
                        );
                    }
                    continue;
                }

                let team = coach
                    .and_then(|c| c.team_assignments.get(&track.track_id))
                    .copied();
                let color = match team {
                    Some(t) => team_color_for(t),
                    None => colors::TEXT_PRIMARY,
                };

                let is_weakest = highlight.weakest_id == Some(track.track_id);
                let stroke_w = if is_weakest { 2.5 } else { 1.25 };
                painter.rect_stroke(
                    pr,
                    1.0,
                    egui::Stroke::new(stroke_w, color),
                    egui::StrokeKind::Outside,
                );
                if is_weakest {
                    painter.rect_stroke(
                        pr.expand(3.0),
                        2.0,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 200, 60)),
                        egui::StrokeKind::Outside,
                    );
                }

                // ID label, only show if room
                if pr.width() > 24.0 {
                    painter.text(
                        pr.left_top() + egui::vec2(2.0, -2.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("#{}", track.track_id),
                        egui::FontId::monospace(9.0),
                        color,
                    );
                }
            }
        }

        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, colors::TEXT_SECONDARY),
            egui::StrokeKind::Outside,
        );
    });
}

/// Project a bbox in texture-pixel coordinates onto a screen rect given
/// the UV window currently displayed in that rect. Returns None if the
/// bbox lies fully outside the visible UV window.
fn project_bbox(
    bbox: crate::detection::BBox,
    tex_size: egui::Vec2,
    uv: egui::Rect,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    let tw = tex_size.x.max(1.0);
    let th = tex_size.y.max(1.0);
    let u1 = bbox.x1 / tw;
    let v1 = bbox.y1 / th;
    let u2 = bbox.x2 / tw;
    let v2 = bbox.y2 / th;
    let uv_w = (uv.max.x - uv.min.x).max(1e-6);
    let uv_h = (uv.max.y - uv.min.y).max(1e-6);
    // Reject if fully outside the UV window (with small margin).
    if u2 < uv.min.x || u1 > uv.max.x || v2 < uv.min.y || v1 > uv.max.y {
        return None;
    }
    let to_x = |u: f32| rect.left() + ((u - uv.min.x) / uv_w) * rect.width();
    let to_y = |v: f32| rect.top() + ((v - uv.min.y) / uv_h) * rect.height();
    let x1 = to_x(u1 as f32).clamp(rect.left(), rect.right());
    let y1 = to_y(v1 as f32).clamp(rect.top(), rect.bottom());
    let x2 = to_x(u2 as f32).clamp(rect.left(), rect.right());
    let y2 = to_y(v2 as f32).clamp(rect.top(), rect.bottom());
    Some(egui::Rect::from_min_max(
        egui::pos2(x1.min(x2), y1.min(y2)),
        egui::pos2(x1.max(x2), y1.max(y2)),
    ))
}
