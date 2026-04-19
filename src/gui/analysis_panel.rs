// gui/analysis_panel.rs — Right-side panel: controls + analysis tabs

use super::app::{AnalysisTab, CoachApp};
use super::colors;
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
            shape.team, shape.width_m, shape.depth_m, shape.line_height_m, shape.compactness_m2
        ));
    }

    if !metrics.coach_metrics.team_colors.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Team Color Signatures").strong());
        for (team, color) in &metrics.coach_metrics.team_colors {
            ui.label(format!(
                "{} | rgb {:.2}/{:.2}/{:.2} | sat {:.2}",
                team, color.r, color.g, color.b, color.saturation
            ));
        }
    }

    if !metrics.coach_metrics.team_lines.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Line / Unit Analysis").strong());
        for lines in metrics.coach_metrics.team_lines.values() {
            ui.label(format!(
                "{} | back {:.1}m | mid {:.1}m | front {:.1}m | B-M {:.1}m | M-F {:.1}m | between-lines {:.0}%",
                lines.team,
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
                    sample.team,
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
                    sample.team,
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
            ui.label(egui::RichText::new(format!("{} | {}", alert.team, alert.title)).strong());
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

    let team_assignments = app
        .metrics
        .as_ref()
        .map(|m| &m.coach_metrics.team_assignments);
    super::pitch_overlay::draw_pitch_overlay(ui, frame_tracks, mapper, team_assignments);

    // Legend
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let dot_size = 8.0;
        let (rect_a, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_a.center(), dot_size / 2.0, colors::PLAYER_TEAM_A);
        ui.label("Team A");
        ui.add_space(8.0);
        let (rect_b, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_b.center(), dot_size / 2.0, colors::PLAYER_TEAM_B);
        ui.label("Team B");
        ui.add_space(8.0);
        let (rect_ball, _) =
            ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect_ball.center(), dot_size / 2.0, colors::BALL_COLOR);
        ui.label("Ball");
    });
}
