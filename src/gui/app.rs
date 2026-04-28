// gui/app.rs — Main application state and top-level UI

use super::colors;
use crate::detection::yolo::YoloDetector;
use crate::detection::{FrameDetections, COCO_PERSON, COCO_SPORTS_BALL};
use crate::library::{LibraryItem, MediaLibrary};
use crate::live_capture::{CaptureSource, CaptureTarget, ScreenCapture};
use crate::metrics::ClipMetrics;
use crate::pitch_awareness::{
    auto_calibration_from_scene, build_detection_sampling_plan, detect_relevant_segments,
    expand_sampled_frame_indices, filter_pitch_frames, infer_scene_awareness,
    is_tactical_wide_shot, PitchTrimSuggestion, SceneAwareness,
};
use crate::pitch_mapping::{HomographyCalibration, PitchMapper, PitchReferencePoint};
use crate::tactical_insights::TacticalInsight;
use crate::tracker::{FrameTracks, TrackingResult};
use crate::video_processor::{VideoFrame, VideoInfo};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use log::info;
use std::collections::VecDeque;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const LIVE_HISTORY_LIMIT: usize = 180;

#[derive(Debug, Clone)]
pub struct LiveAnalysisSnapshot {
    pub frame: VideoFrame,
    pub video_info: VideoInfo,
    pub detections: FrameDetections,
    pub tracks: FrameTracks,
    pub players_visible: usize,
    pub ball_visible: bool,
    pub tracked_players: usize,
    pub inferred_fps: f32,
}

#[derive(Debug, Clone, Default)]
pub struct LiveStats {
    pub frames_processed: u64,
    pub current_players_visible: usize,
    pub current_tracked_players: usize,
    pub ball_visible: bool,
    pub inferred_fps: f32,
    pub last_timestamp_secs: f64,
}

#[derive(Debug, Clone, Default)]
pub struct LiveModeState {
    pub enabled: bool,
    pub requested: bool,
    pub stats: LiveStats,
    pub sources: Vec<CaptureSource>,
}

/// Analysis progress state
#[derive(Debug, Clone)]
pub enum AnalysisState {
    Idle,
    DownloadingVideo,
    LoadingVideo,
    RunningDetection { progress: f32 },
    RunningTracker,
    ComputingMetrics,
    GeneratingInsights,
    Complete,
    Error(String),
}

/// Messages from background analysis thread
enum AnalysisMessage {
    StateUpdate(AnalysisState),
    VideoDownloaded(PathBuf),
    VideoLoaded {
        info: VideoInfo,
        frames: Vec<VideoFrame>,
    },
    DetectionsDone(Vec<FrameDetections>),
    TrackingDone(TrackingResult),
    SceneAwarenessReady(SceneAwareness),
    TrimSuggestionReady(PitchTrimSuggestion),
    MapperReady(PitchMapper),
    MetricsDone(ClipMetrics),
    InsightsDone(Vec<TacticalInsight>),
    LiveFrame(LiveAnalysisSnapshot),
    Error(String),
}

/// Active tab in the analysis panel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisTab {
    Coach,
    MatchPlan,
    FourView,
    Tracking,
    Heatmaps,
    PitchView,
    Insights,
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCookieSource {
    Chrome,
    Safari,
    Firefox,
}

impl BrowserCookieSource {
    fn yt_dlp_name(self) -> &'static str {
        match self {
            BrowserCookieSource::Chrome => "chrome",
            BrowserCookieSource::Safari => "safari",
            BrowserCookieSource::Firefox => "firefox",
        }
    }

    fn label(self) -> &'static str {
        match self {
            BrowserCookieSource::Chrome => "Chrome",
            BrowserCookieSource::Safari => "Safari",
            BrowserCookieSource::Firefox => "Firefox",
        }
    }
}

/// Main app state
pub struct CoachApp {
    // Video state
    pub video_path: Option<PathBuf>,
    pub video_info: Option<VideoInfo>,
    pub frames: Vec<VideoFrame>,
    pub current_frame_idx: usize,
    pub analysis_start_idx: usize,
    pub analysis_end_idx: usize,
    pub playback_trimmed_only: bool,
    pub frame_texture: Option<egui::TextureHandle>,
    pub is_playing: bool,
    pub last_frame_time: f64,

    // Model
    pub model_path: Option<PathBuf>,

    // Analysis state
    pub analysis_state: AnalysisState,
    pub detections: Vec<FrameDetections>,
    pub tracking: Option<TrackingResult>,
    pub metrics: Option<ClipMetrics>,
    pub insights: Vec<TacticalInsight>,
    pub mapper: Option<PitchMapper>,
    pub auto_pitch_ready: bool,
    pub scene_awareness: Option<SceneAwareness>,
    pub trim_suggestion: Option<PitchTrimSuggestion>,
    pub use_trimmed_segments: bool,

    // Manual pitch calibration
    pub homography_calibration: HomographyCalibration,
    pub editing_corners: bool,
    pub selected_reference_point: usize,

    // UI state
    pub active_tab: AnalysisTab,
    pub show_bboxes: bool,
    pub show_trails: bool,
    pub show_ids: bool,
    pub selected_player: Option<u32>,
    pub library: MediaLibrary,
    pub library_status: Option<String>,

    // Background processing channel
    msg_tx: Sender<AnalysisMessage>,
    msg_rx: Receiver<AnalysisMessage>,

    // Detection confidence threshold
    pub conf_threshold: f32,

    // YouTube URL input
    pub url_input: String,
    pub use_browser_cookies_for_url_loader: bool,
    pub browser_cookie_source: BrowserCookieSource,

    // Live capture state
    pub live_mode: LiveModeState,
    pub live_detection_stride: u32,
    pub live_source_index: usize,
    live_history: VecDeque<LiveAnalysisSnapshot>,
    live_capture_running: Arc<AtomicBool>,
}

impl CoachApp {
    fn default_model_path() -> Option<PathBuf> {
        let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/yolov8n.onnx");
        candidate.exists().then_some(candidate)
    }

    pub fn current_frame(&self) -> Option<&VideoFrame> {
        self.frames.get(self.current_frame_idx)
    }

    pub fn analysis_range(&self) -> Option<(usize, usize)> {
        if self.frames.is_empty() {
            return None;
        }

        let max_idx = self.frames.len() - 1;
        let start = self.analysis_start_idx.min(max_idx);
        let end = self.analysis_end_idx.min(max_idx);
        Some((start.min(end), start.max(end)))
    }

    pub fn enabled_trim_segments(&self) -> Vec<&crate::pitch_awareness::RelevantSegment> {
        self.trim_suggestion
            .as_ref()
            .map(|trim| {
                trim.segments
                    .iter()
                    .filter(|segment| segment.enabled)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn trimmed_frame_indices(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        for segment in self.enabled_trim_segments() {
            indices.extend(segment.start_idx..=segment.end_idx);
        }
        indices
    }

    pub fn displayed_frame_count(&self) -> usize {
        if self.playback_trimmed_only {
            let trimmed = self.trimmed_frame_indices();
            if !trimmed.is_empty() {
                return trimmed.len();
            }
        }

        self.frames.len()
    }

    pub fn displayed_frame_position(&self) -> usize {
        if self.playback_trimmed_only {
            let trimmed = self.trimmed_frame_indices();
            if let Some(pos) = trimmed
                .iter()
                .position(|idx| *idx == self.current_frame_idx)
            {
                return pos;
            }
        }

        self.current_frame_idx
    }

    pub fn set_displayed_frame_position(&mut self, displayed_idx: usize) {
        if self.playback_trimmed_only {
            let trimmed = self.trimmed_frame_indices();
            if let Some(frame_idx) = trimmed.get(displayed_idx) {
                self.current_frame_idx = *frame_idx;
                return;
            }
        }

        if !self.frames.is_empty() {
            self.current_frame_idx = displayed_idx.min(self.frames.len() - 1);
        }
    }

    pub fn advance_playback_frame(&mut self) {
        if self.frames.is_empty() {
            return;
        }

        if self.playback_trimmed_only {
            let trimmed = self.trimmed_frame_indices();
            if let Some(pos) = trimmed
                .iter()
                .position(|idx| *idx == self.current_frame_idx)
            {
                let next_pos = (pos + 1) % trimmed.len();
                self.current_frame_idx = trimmed[next_pos];
                return;
            }

            if let Some(first_idx) = trimmed.first() {
                self.current_frame_idx = *first_idx;
                return;
            }
        }

        self.current_frame_idx = (self.current_frame_idx + 1) % self.frames.len();
    }

    pub fn jump_to_segment(&mut self, segment_idx: usize) {
        if let Some(segment) = self
            .trim_suggestion
            .as_ref()
            .and_then(|trim| trim.segments.get(segment_idx))
        {
            self.current_frame_idx = segment.start_idx;
            self.is_playing = false;
        }
    }

    pub fn normalize_trim_segments(&mut self) {
        let Some(trim) = &mut self.trim_suggestion else {
            return;
        };

        if self.frames.is_empty() {
            return;
        }

        for segment in &mut trim.segments {
            let max_idx = self.frames.len() - 1;
            segment.start_idx = segment.start_idx.min(max_idx);
            segment.end_idx = segment.end_idx.min(max_idx);
            if segment.end_idx < segment.start_idx {
                std::mem::swap(&mut segment.start_idx, &mut segment.end_idx);
            }
            segment.start_secs = self.frames[segment.start_idx].timestamp_secs;
            segment.end_secs = self.frames[segment.end_idx].timestamp_secs;
        }

        trim.segments.sort_by_key(|segment| segment.start_idx);

        for i in 1..trim.segments.len() {
            let prev_end = trim.segments[i - 1].end_idx;
            if trim.segments[i].start_idx <= prev_end {
                trim.segments[i].start_idx = (prev_end + 1).min(self.frames.len() - 1);
                if trim.segments[i].end_idx < trim.segments[i].start_idx {
                    trim.segments[i].end_idx = trim.segments[i].start_idx;
                }
                trim.segments[i].start_secs =
                    self.frames[trim.segments[i].start_idx].timestamp_secs;
                trim.segments[i].end_secs = self.frames[trim.segments[i].end_idx].timestamp_secs;
            }
        }
    }

    fn trim_suggestion_for_range(
        &self,
        start_idx: usize,
        end_idx: usize,
    ) -> Option<PitchTrimSuggestion> {
        let trim = self.trim_suggestion.as_ref()?;
        let mut segments = Vec::new();

        for segment in &trim.segments {
            let overlap_start = segment.start_idx.max(start_idx);
            let overlap_end = segment.end_idx.min(end_idx);
            if overlap_start > overlap_end {
                continue;
            }

            segments.push(crate::pitch_awareness::RelevantSegment {
                start_idx: overlap_start - start_idx,
                end_idx: overlap_end - start_idx,
                start_secs: self.frames[overlap_start].timestamp_secs,
                end_secs: self.frames[overlap_end].timestamp_secs,
                avg_confidence: segment.avg_confidence,
                enabled: segment.enabled,
            });
        }

        if segments.is_empty() {
            return None;
        }

        let kept_frames = segments
            .iter()
            .filter(|segment| segment.enabled)
            .map(|segment| segment.end_idx - segment.start_idx + 1)
            .sum();
        let kept_duration_secs = segments
            .iter()
            .filter(|segment| segment.enabled)
            .map(|segment| (segment.end_secs - segment.start_secs).max(0.0))
            .sum();

        Some(PitchTrimSuggestion {
            segments,
            representative_scene: trim.representative_scene.clone(),
            kept_frames,
            kept_duration_secs,
        })
    }

    fn save_current_trim_to_library(&mut self) {
        let Some(source_path) = self.video_path.clone() else {
            self.library_status = Some("No source video loaded".to_string());
            return;
        };

        let Some(trim) = &self.trim_suggestion else {
            self.library_status = Some("No trim segments available".to_string());
            return;
        };

        let enabled_segments: Vec<_> = trim
            .segments
            .iter()
            .filter(|segment| segment.enabled)
            .collect();
        if enabled_segments.is_empty() {
            self.library_status = Some("No enabled trim segments to save".to_string());
            return;
        }

        let mut manifest = String::new();
        manifest.push_str("TRIMMED CLIP MANIFEST\n");
        manifest.push_str(&format!("Source: {}\n", source_path.display()));
        manifest.push_str(&format!("Created: {}\n\n", chrono::Utc::now().to_rfc3339()));
        for (idx, segment) in enabled_segments.iter().enumerate() {
            manifest.push_str(&format!(
                "Segment {}: {:.2}s -> {:.2}s (frames {}-{})\n",
                idx + 1,
                segment.start_secs,
                segment.end_secs,
                segment.start_idx,
                segment.end_idx,
            ));
        }

        let duration_secs: f64 = enabled_segments
            .iter()
            .map(|segment| (segment.end_secs - segment.start_secs).max(0.0))
            .sum();
        let segment_ranges: Vec<(f64, f64)> = enabled_segments
            .iter()
            .map(|segment| (segment.start_secs, segment.end_secs))
            .collect();
        let title = format!(
            "{} trim {}",
            source_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy(),
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );

        match self.library.save_trimmed_clip(
            &source_path,
            &title,
            &manifest,
            &segment_ranges,
            Some(duration_secs),
        ) {
            Ok(item) => {
                self.library_status = Some(format!("Saved trimmed clip: {}", item.title));
            }
            Err(e) => {
                self.library_status = Some(format!("Failed to save trimmed clip: {}", e));
            }
        }
    }

    pub fn load_library_item(&mut self, item: &LibraryItem) {
        if !item.file_path.exists() {
            self.library_status = Some(format!(
                "Library file missing: {}",
                item.file_path.display()
            ));
            return;
        }

        self.video_path = Some(item.file_path.clone());
        self.library_status = Some(format!("Loading from library: {}", item.title));
        self.load_video(item.file_path.clone());
    }

    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let library = MediaLibrary::load_or_create().unwrap_or_else(|e| {
            log::error!("Failed to initialize media library: {}", e);
            MediaLibrary::load_or_create().expect("library fallback init")
        });
        let live_sources = ScreenCapture::available_sources().unwrap_or_else(|err| {
            log::warn!("Failed to enumerate live capture sources: {}", err);
            vec![CaptureSource {
                target: CaptureTarget::Display(1),
                label: "Main Display".to_string(),
            }]
        });

        Self {
            video_path: None,
            video_info: None,
            frames: Vec::new(),
            current_frame_idx: 0,
            analysis_start_idx: 0,
            analysis_end_idx: 0,
            playback_trimmed_only: true,
            frame_texture: None,
            is_playing: false,
            last_frame_time: 0.0,

            model_path: Self::default_model_path(),

            analysis_state: AnalysisState::Idle,
            detections: Vec::new(),
            tracking: None,
            metrics: None,
            insights: Vec::new(),
            mapper: None,
            auto_pitch_ready: false,
            scene_awareness: None,
            trim_suggestion: None,
            use_trimmed_segments: true,

            homography_calibration: HomographyCalibration::default(),
            editing_corners: false,
            selected_reference_point: 0,

            active_tab: AnalysisTab::MatchPlan,
            show_bboxes: true,
            show_trails: true,
            show_ids: true,
            selected_player: None,
            library,
            library_status: None,

            msg_tx: tx,
            msg_rx: rx,

            conf_threshold: 0.35,

            url_input: String::new(),
            use_browser_cookies_for_url_loader: false,
            browser_cookie_source: BrowserCookieSource::Chrome,

            live_mode: LiveModeState {
                enabled: false,
                requested: false,
                stats: LiveStats::default(),
                sources: live_sources,
            },
            live_detection_stride: 2,
            live_source_index: 0,
            live_history: VecDeque::with_capacity(LIVE_HISTORY_LIMIT),
            live_capture_running: Arc::new(AtomicBool::new(false)),
        }
    }

    fn reset_live_view(&mut self) {
        self.frames.clear();
        self.current_frame_idx = 0;
        self.frame_texture = None;
        self.video_info = None;
        self.detections.clear();
        self.tracking = None;
        self.metrics = None;
        self.insights.clear();
        self.scene_awareness = None;
        self.trim_suggestion = None;
        self.mapper = None;
        self.auto_pitch_ready = false;
        self.analysis_start_idx = 0;
        self.analysis_end_idx = 0;
        self.is_playing = false;
        self.live_history.clear();
        self.live_mode.stats = LiveStats::default();
    }

    pub fn toggle_live_capture(&mut self) {
        if self.live_mode.enabled {
            self.live_capture_running.store(false, Ordering::Relaxed);
            self.live_mode.enabled = false;
            self.live_mode.requested = false;
            self.analysis_state = AnalysisState::Idle;
            return;
        }

        if self.model_path.is_none() {
            self.analysis_state =
                AnalysisState::Error("Load an ONNX model before starting live capture.".into());
            return;
        }

        if self.live_mode.sources.is_empty() {
            self.analysis_state =
                AnalysisState::Error("No live capture sources were found on this machine.".into());
            return;
        }

        self.reset_live_view();
        self.live_mode.enabled = true;
        self.live_mode.requested = true;
        self.live_capture_running.store(true, Ordering::Relaxed);
        self.analysis_state = AnalysisState::RunningDetection { progress: 0.0 };
        self.start_live_capture();
    }

    fn start_live_capture(&self) {
        let tx = self.msg_tx.clone();
        let model_path = self.model_path.clone();
        let conf_threshold = self.conf_threshold;
        let target = self
            .live_mode
            .sources
            .get(self.live_source_index)
            .map(|source| source.target.clone())
            .unwrap_or(CaptureTarget::Display(1));
        let stride = self.live_detection_stride.max(1);
        let running = Arc::clone(&self.live_capture_running);

        std::thread::spawn(move || {
            let Some(model_path) = model_path else {
                tx.send(AnalysisMessage::Error(
                    "Load an ONNX model before starting live capture.".into(),
                ))
                .ok();
                return;
            };

            let mut capture = ScreenCapture::new(target, 1280);
            let mut detector = match YoloDetector::new(&model_path, conf_threshold, 0.45) {
                Ok(detector) => detector,
                Err(err) => {
                    tx.send(AnalysisMessage::Error(format!(
                        "Live model load failed: {}",
                        err
                    )))
                    .ok();
                    return;
                }
            };
            let mut tracker = crate::tracker::bytetrack::ByteTracker::with_params(15, 1, 0.2);
            let mut latest_detections: Option<FrameDetections> = None;
            let mut frame_counter = 0u64;
            let mut last_frame_at = std::time::Instant::now();

            while running.load(Ordering::Relaxed) {
                match capture.capture_frame() {
                    Ok((video_info, frame)) => {
                        let now = std::time::Instant::now();
                        let inferred_fps = if frame_counter == 0 {
                            0.0
                        } else {
                            let dt = now.duration_since(last_frame_at).as_secs_f32();
                            if dt > 0.0 {
                                1.0 / dt
                            } else {
                                0.0
                            }
                        };
                        last_frame_at = now;

                        let detections = if frame_counter % stride as u64 == 0
                            || latest_detections.is_none()
                        {
                            match detector.detect_frame(&frame) {
                                Ok(mut dets) => {
                                    dets.frame_index = frame.index;
                                    dets.timestamp_secs = frame.timestamp_secs;
                                    latest_detections = Some(dets.clone());
                                    dets
                                }
                                Err(err) => {
                                    tx.send(AnalysisMessage::Error(format!(
                                        "Live detection failed: {}",
                                        err
                                    )))
                                    .ok();
                                    break;
                                }
                            }
                        } else {
                            let mut dets = latest_detections.clone().unwrap_or(FrameDetections {
                                frame_index: frame.index,
                                timestamp_secs: frame.timestamp_secs,
                                detections: Vec::new(),
                            });
                            dets.frame_index = frame.index;
                            dets.timestamp_secs = frame.timestamp_secs;
                            dets
                        };

                        let tracked_objects = tracker.update(&detections.detections);
                        let tracks = FrameTracks {
                            frame_index: frame.index,
                            timestamp_secs: frame.timestamp_secs,
                            tracks: tracked_objects,
                        };

                        let players_visible = detections
                            .detections
                            .iter()
                            .filter(|d| d.class_id == COCO_PERSON)
                            .count();
                        let ball_visible = detections
                            .detections
                            .iter()
                            .any(|d| d.class_id == COCO_SPORTS_BALL);
                        let tracked_players = tracks
                            .tracks
                            .iter()
                            .filter(|track| track.class_id == COCO_PERSON)
                            .count();

                        let snapshot = LiveAnalysisSnapshot {
                            frame,
                            video_info,
                            detections,
                            tracks,
                            players_visible,
                            ball_visible,
                            tracked_players,
                            inferred_fps,
                        };

                        if tx.send(AnalysisMessage::LiveFrame(snapshot)).is_err() {
                            break;
                        }

                        frame_counter += 1;
                        std::thread::sleep(Duration::from_millis(70));
                    }
                    Err(err) => {
                        tx.send(AnalysisMessage::Error(format!(
                            "Live capture failed: {}",
                            err
                        )))
                        .ok();
                        break;
                    }
                }
            }

            running.store(false, Ordering::Relaxed);
        });
    }

    fn apply_live_snapshot(&mut self, snapshot: LiveAnalysisSnapshot) {
        self.analysis_state = AnalysisState::RunningDetection { progress: 0.0 };
        self.video_info = Some(snapshot.video_info.clone());
        self.frames = vec![snapshot.frame.clone()];
        self.current_frame_idx = 0;
        self.analysis_start_idx = 0;
        self.analysis_end_idx = 0;
        self.detections = vec![snapshot.detections.clone()];
        self.tracking = Some(TrackingResult {
            frame_tracks: vec![snapshot.tracks.clone()],
            track_classes: snapshot
                .tracks
                .tracks
                .iter()
                .map(|track| (track.track_id, track.class_name.clone()))
                .collect(),
            total_tracks: snapshot.tracks.tracks.len() as u32,
        });

        self.live_mode.stats.frames_processed += 1;
        self.live_mode.stats.current_players_visible = snapshot.players_visible;
        self.live_mode.stats.current_tracked_players = snapshot.tracked_players;
        self.live_mode.stats.ball_visible = snapshot.ball_visible;
        self.live_mode.stats.inferred_fps = snapshot.inferred_fps;
        self.live_mode.stats.last_timestamp_secs = snapshot.frame.timestamp_secs;

        if self.live_history.len() == LIVE_HISTORY_LIMIT {
            self.live_history.pop_front();
        }
        self.live_history.push_back(snapshot);
    }

    /// Process messages from background thread
    fn process_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                AnalysisMessage::StateUpdate(state) => {
                    self.analysis_state = state;
                }
                AnalysisMessage::VideoDownloaded(path) => {
                    info!("YouTube video downloaded to: {}", path.display());
                    self.video_path = Some(path.clone());
                    self.load_video(path);
                }
                AnalysisMessage::VideoLoaded { info, frames } => {
                    info!("Video loaded: {} frames", frames.len());
                    if let Some(path) = &self.video_path {
                        match self
                            .library
                            .register_existing_original(path, Some(info.duration_secs))
                        {
                            Ok(item) => {
                                self.library_status =
                                    Some(format!("Library registered: {}", item.title));
                            }
                            Err(e) => {
                                self.library_status =
                                    Some(format!("Library register failed: {}", e));
                            }
                        }
                    }
                    self.video_info = Some(info);
                    self.frames = frames;
                    self.current_frame_idx = 0;
                    self.analysis_start_idx = 0;
                    self.analysis_end_idx = self.frames.len().saturating_sub(1);
                    self.detections.clear();
                    self.tracking = None;
                    self.metrics = None;
                    self.insights.clear();
                    self.mapper = None;
                    self.auto_pitch_ready = false;
                    self.scene_awareness = None;
                    self.trim_suggestion =
                        Some(detect_relevant_segments(&self.frames, 10, 0.42, 1.5, 20));
                    self.homography_calibration = HomographyCalibration::default();
                    self.selected_reference_point = 0;

                    if let Some(frame) = self.frames.first() {
                        if let Some(scene) = infer_scene_awareness(frame) {
                            self.scene_awareness = Some(scene.clone());
                            self.homography_calibration = auto_calibration_from_scene(&scene);
                            self.auto_pitch_ready = true;
                        }
                    }
                }
                AnalysisMessage::DetectionsDone(dets) => {
                    info!("Detections received: {} frames", dets.len());
                    self.detections = dets;
                }
                AnalysisMessage::TrackingDone(result) => {
                    info!("Tracking received: {} tracks", result.total_tracks);
                    self.tracking = Some(result);
                }
                AnalysisMessage::SceneAwarenessReady(scene) => {
                    self.scene_awareness = Some(scene);
                    self.auto_pitch_ready = true;
                }
                AnalysisMessage::TrimSuggestionReady(trim) => {
                    self.trim_suggestion = Some(trim);
                }
                AnalysisMessage::MetricsDone(m) => {
                    self.metrics = Some(m);
                }
                AnalysisMessage::MapperReady(m) => {
                    info!("Pitch mapper received");
                    self.mapper = Some(m);
                }
                AnalysisMessage::InsightsDone(ins) => {
                    self.insights = ins;
                    self.analysis_state = AnalysisState::Complete;
                }
                AnalysisMessage::LiveFrame(snapshot) => {
                    if self.live_mode.enabled {
                        self.apply_live_snapshot(snapshot);
                    }
                }
                AnalysisMessage::Error(e) => {
                    self.live_capture_running.store(false, Ordering::Relaxed);
                    self.live_mode.enabled = false;
                    self.live_mode.requested = false;
                    self.analysis_state = AnalysisState::Error(e);
                }
            }
        }
    }

    /// Load video in background
    fn load_video(&self, path: PathBuf) {
        let tx = self.msg_tx.clone();
        std::thread::spawn(move || {
            tx.send(AnalysisMessage::StateUpdate(AnalysisState::LoadingVideo))
                .ok();

            match crate::video_processor::decode_all_frames(&path, Some(3000)) {
                Ok((info, frames)) => {
                    tx.send(AnalysisMessage::VideoLoaded { info, frames }).ok();
                    tx.send(AnalysisMessage::StateUpdate(AnalysisState::Idle))
                        .ok();
                }
                Err(e) => {
                    tx.send(AnalysisMessage::Error(format!("Video load failed: {}", e)))
                        .ok();
                }
            }
        });
    }

    /// Download a YouTube video via yt-dlp in background, then load it
    fn download_youtube(&self, url: String) {
        let tx = self.msg_tx.clone();
        let use_browser_cookies = self.use_browser_cookies_for_url_loader;
        let browser_cookie_source = self.browser_cookie_source;
        std::thread::spawn(move || {
            tx.send(AnalysisMessage::StateUpdate(
                AnalysisState::DownloadingVideo,
            ))
            .ok();

            // Create a temp directory for the download
            let download_dir = std::env::temp_dir().join("football-coach-poc");
            if let Err(e) = std::fs::create_dir_all(&download_dir) {
                tx.send(AnalysisMessage::Error(format!(
                    "Failed to create temp dir: {}",
                    e
                )))
                .ok();
                return;
            }

            let output_template = download_dir.join("%(title).50s.%(ext)s");

            info!("Downloading YouTube video: {}", url);

            let output_template_str = output_template.to_string_lossy().into_owned();
            let browser_name = browser_cookie_source.yt_dlp_name();
            let run_download = |with_cookies: bool| {
                let mut command = std::process::Command::new("yt-dlp");
                command.args([
                    "--no-playlist",
                    "-f",
                    "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720][ext=mp4]/best[height<=720]/best",
                    "--merge-output-format",
                    "mp4",
                    "--no-overwrites",
                    "-o",
                    output_template_str.as_str(),
                ]);
                if with_cookies {
                    command.args(["--cookies-from-browser", browser_name]);
                }
                command.arg(&url);
                command.output()
            };

            let mut attempted_with_cookies = use_browser_cookies;
            let mut result = run_download(use_browser_cookies);

            if !attempted_with_cookies {
                if let Ok(output) = &result {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
                        let needs_cookies = stderr.contains("sign in to confirm you're not a bot")
                            || stderr.contains("--cookies-from-browser")
                            || stderr.contains("use --cookies")
                            || stderr.contains("authentication");
                        if needs_cookies {
                            info!(
                                "Retrying yt-dlp with browser cookies from {}",
                                browser_cookie_source.label()
                            );
                            result = run_download(true);
                            attempted_with_cookies = true;
                        }
                    }
                }
            }

            match result {
                Ok(output) => {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        // Parse the output to find the downloaded file path
                        // yt-dlp prints "[download] Destination: <path>" or
                        // "[download] <path> has already been downloaded"
                        let downloaded_path = stdout
                            .lines()
                            .filter_map(|line| {
                                if line.contains("[download] Destination:") {
                                    line.split("Destination:")
                                        .nth(1)
                                        .map(|s| s.trim().to_string())
                                } else if line.contains("has already been downloaded") {
                                    let path = line
                                        .trim_start_matches("[download] ")
                                        .trim_end_matches(" has already been downloaded");
                                    Some(path.to_string())
                                } else if line.contains("[Merger] Merging formats into") {
                                    line.split('"').nth(1).map(|s| s.to_string())
                                } else {
                                    None
                                }
                            })
                            .last();

                        if let Some(path_str) = downloaded_path {
                            let path = PathBuf::from(&path_str);
                            if path.exists() {
                                info!("Download complete: {}", path.display());
                                tx.send(AnalysisMessage::VideoDownloaded(path)).ok();
                                return;
                            }
                        }

                        // Fallback: find the most recent mp4 in the download dir
                        if let Ok(entries) = std::fs::read_dir(&download_dir) {
                            let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().and_then(|e| e.to_str()) == Some("mp4") {
                                    if let Ok(meta) = path.metadata() {
                                        if let Ok(modified) = meta.modified() {
                                            if newest.as_ref().map_or(true, |(_, t)| modified > *t)
                                            {
                                                newest = Some((path, modified));
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some((path, _)) = newest {
                                info!("Download complete (fallback): {}", path.display());
                                tx.send(AnalysisMessage::VideoDownloaded(path)).ok();
                                return;
                            }
                        }

                        tx.send(AnalysisMessage::Error(
                            "yt-dlp succeeded but could not find downloaded file".into(),
                        ))
                        .ok();
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let cookie_hint = if attempted_with_cookies {
                            format!(
                                " Tried browser cookies from {}.",
                                browser_cookie_source.label()
                            )
                        } else {
                            String::new()
                        };
                        tx.send(AnalysisMessage::Error(format!(
                            "yt-dlp failed: {}{}",
                            stderr.lines().last().unwrap_or("unknown error"),
                            cookie_hint
                        )))
                        .ok();
                    }
                }
                Err(e) => {
                    tx.send(AnalysisMessage::Error(format!(
                        "Failed to run yt-dlp (is it installed?): {}",
                        e
                    )))
                    .ok();
                }
            }
        });
    }

    /// Run the full analysis pipeline in background
    fn run_analysis(&self) {
        let tx = self.msg_tx.clone();
        let (start_idx, end_idx) = match self.analysis_range() {
            Some(range) => range,
            None => {
                tx.send(AnalysisMessage::Error(
                    "No analysis range selected. Load a video first.".into(),
                ))
                .ok();
                return;
            }
        };
        let frames = self.frames[start_idx..=end_idx].to_vec();
        let model_path = self.model_path.clone();
        let calibration = self.homography_calibration.clone();
        let conf_threshold = self.conf_threshold;
        let fps = self.video_info.as_ref().map(|v| v.fps).unwrap_or(30.0);
        let use_trimmed_segments = self.use_trimmed_segments;
        let trim_suggestion = self.trim_suggestion_for_range(start_idx, end_idx);

        std::thread::spawn(move || {
            let analysis_result = panic::catch_unwind(AssertUnwindSafe(|| {
                // Step 1: Load YOLO model
                let model_path = match model_path {
                    Some(p) => p,
                    None => {
                        tx.send(AnalysisMessage::Error(
                            "No ONNX model selected. Please load a YOLOv8 .onnx file.".into(),
                        ))
                        .ok();
                        return;
                    }
                };

                tx.send(AnalysisMessage::StateUpdate(
                    AnalysisState::RunningDetection { progress: 0.0 },
                ))
                .ok();

                let mut detector = match YoloDetector::new(&model_path, conf_threshold, 0.45) {
                    Ok(d) => d,
                    Err(e) => {
                        tx.send(AnalysisMessage::Error(format!("Model load failed: {}", e)))
                            .ok();
                        return;
                    }
                };

                let scene = infer_scene_awareness(&frames[0]);
                let pretrim_frames = if use_trimmed_segments {
                    if let Some(trim) = trim_suggestion {
                        let enabled_segments: Vec<_> = trim
                            .segments
                            .into_iter()
                            .filter(|segment| segment.enabled)
                            .collect();
                        if !enabled_segments.is_empty() {
                            let mut kept = Vec::new();
                            for segment in enabled_segments {
                                kept.extend_from_slice(
                                    &frames[segment.start_idx..=segment.end_idx],
                                );
                            }
                            kept
                        } else if let Some(scene) = scene.as_ref() {
                            tx.send(AnalysisMessage::SceneAwarenessReady(scene.clone()))
                                .ok();
                            filter_pitch_frames(&frames, scene, 0.42)
                        } else {
                            frames.clone()
                        }
                    } else if let Some(scene) = scene.as_ref() {
                        tx.send(AnalysisMessage::SceneAwarenessReady(scene.clone()))
                            .ok();
                        filter_pitch_frames(&frames, scene, 0.42)
                    } else {
                        frames.clone()
                    }
                } else {
                    frames.clone()
                };

                if pretrim_frames.is_empty() {
                    tx.send(AnalysisMessage::Error(
                        "No pitch-visible frames detected in the selected range.".into(),
                    ))
                    .ok();
                    return;
                }

                let sampled_frames = build_detection_sampling_plan(&pretrim_frames, 6);
                let sampled_detections = match detector.detect_frames(&sampled_frames, |progress| {
                    tx.send(AnalysisMessage::StateUpdate(
                        AnalysisState::RunningDetection {
                            progress: progress * 0.35,
                        },
                    ))
                    .ok();
                }) {
                    Ok(d) => d,
                    Err(e) => {
                        tx.send(AnalysisMessage::Error(format!("Detection failed: {}", e)))
                            .ok();
                        return;
                    }
                };

                let sampled_keep_indices: Vec<usize> = sampled_frames
                    .iter()
                    .enumerate()
                    .filter_map(|(i, frame)| {
                        let scene = infer_scene_awareness(frame);
                        let keep = is_tactical_wide_shot(
                            frame,
                            scene.as_ref(),
                            sampled_detections.get(i),
                            0.52,
                        );
                        keep.then_some(i * 6)
                    })
                    .collect();

                let expanded_indices =
                    expand_sampled_frame_indices(&sampled_keep_indices, pretrim_frames.len(), 3);
                let analysis_frames: Vec<_> = if expanded_indices.is_empty() {
                    pretrim_frames
                        .iter()
                        .filter(|frame| {
                            let scene = infer_scene_awareness(frame);
                            scene.as_ref().map(|scene| scene.confidence).unwrap_or(0.0) >= 0.55
                        })
                        .cloned()
                        .collect()
                } else {
                    expanded_indices
                        .into_iter()
                        .filter_map(|idx| pretrim_frames.get(idx).cloned())
                        .collect()
                };

                if analysis_frames.is_empty() {
                    tx.send(AnalysisMessage::Error(
                        "Trim kept no tactical wide shots. Widen the trim or disable auto-trim."
                            .into(),
                    ))
                    .ok();
                    return;
                }

                // Step 2: Run detection on all frames
                let detections = match detector.detect_frames(&analysis_frames, |progress| {
                    tx.send(AnalysisMessage::StateUpdate(
                        AnalysisState::RunningDetection {
                            progress: 0.35 + progress * 0.65,
                        },
                    ))
                    .ok();
                }) {
                    Ok(d) => d,
                    Err(e) => {
                        tx.send(AnalysisMessage::Error(format!("Detection failed: {}", e)))
                            .ok();
                        return;
                    }
                };
                tx.send(AnalysisMessage::DetectionsDone(detections.clone()))
                    .ok();

                // Step 3: Run tracker
                tx.send(AnalysisMessage::StateUpdate(AnalysisState::RunningTracker))
                    .ok();
                let mut tracker = crate::tracker::bytetrack::ByteTracker::with_params(18, 2, 0.25);
                let tracking_result = tracker.track_all(&detections);
                tx.send(AnalysisMessage::TrackingDone(tracking_result.clone()))
                    .ok();

                // Step 4: Compute metrics
                tx.send(AnalysisMessage::StateUpdate(
                    AnalysisState::ComputingMetrics,
                ))
                .ok();

                let effective_calibration = if calibration.is_ready() {
                    calibration
                } else {
                    match scene {
                        Some(scene) => auto_calibration_from_scene(&scene),
                        None => calibration,
                    }
                };

                let mapper = match PitchMapper::from_calibration(effective_calibration) {
                    Ok(m) => m,
                    Err(e) => {
                        tx.send(AnalysisMessage::Error(format!(
                            "Pitch calibration failed: {}. Adjust the selected landmarks.",
                            e
                        )))
                        .ok();
                        return;
                    }
                };

                tx.send(AnalysisMessage::MapperReady(mapper.clone())).ok();

                let metrics = crate::metrics::compute_all_metrics(
                    &tracking_result,
                    &mapper,
                    fps,
                    &analysis_frames,
                );
                tx.send(AnalysisMessage::MetricsDone(metrics.clone())).ok();

                // Step 5: Generate insights
                tx.send(AnalysisMessage::StateUpdate(
                    AnalysisState::GeneratingInsights,
                ))
                .ok();
                let insights = crate::tactical_insights::generate_insights(&metrics);
                tx.send(AnalysisMessage::InsightsDone(insights)).ok();
            }));

            if let Err(panic_payload) = analysis_result {
                let panic_message = if let Some(message) = panic_payload.downcast_ref::<&str>() {
                    (*message).to_string()
                } else if let Some(message) = panic_payload.downcast_ref::<String>() {
                    message.clone()
                } else {
                    "unknown panic".to_string()
                };
                tx.send(AnalysisMessage::Error(format!(
                    "Analysis crashed: {}",
                    panic_message
                )))
                .ok();
            }
        });
    }

    /// Upload frame image to GPU texture
    fn update_frame_texture(&mut self, ctx: &egui::Context) {
        if self.frames.is_empty() {
            return;
        }

        let frame = &self.frames[self.current_frame_idx];
        let size = [frame.image.width() as usize, frame.image.height() as usize];
        let pixels: Vec<egui::Color32> = frame
            .image
            .pixels()
            .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
            .collect();

        let image = egui::ColorImage { size, pixels };

        match &mut self.frame_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.frame_texture =
                    Some(ctx.load_texture("video_frame", image, egui::TextureOptions::LINEAR));
            }
        }
    }

    pub fn selected_reference_point(&self) -> PitchReferencePoint {
        self.homography_calibration.reference_points[self.selected_reference_point]
    }
}

impl eframe::App for CoachApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background messages
        self.process_messages();

        // Request repaint while background work is in progress
        if !matches!(
            self.analysis_state,
            AnalysisState::Idle | AnalysisState::Complete | AnalysisState::Error(_)
        ) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        if self.live_mode.enabled {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        // Auto-advance frames if playing
        if self.is_playing && !self.frames.is_empty() {
            let now = ctx.input(|i| i.time);
            let fps = self.video_info.as_ref().map(|v| v.fps).unwrap_or(30.0);
            let frame_duration = 1.0 / fps;

            if now - self.last_frame_time >= frame_duration {
                self.advance_playback_frame();
                self.last_frame_time = now;
            }
            ctx.request_repaint();
        }

        // Update texture for current frame
        self.update_frame_texture(ctx);

        // Top menu bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.label(
                    egui::RichText::new("Football Coach PoC")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.separator();

                if ui.button("Load Video").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Video", &["mp4", "avi", "mkv", "mov", "webm"])
                        .pick_file()
                    {
                        self.video_path = Some(path.clone());
                        self.load_video(path);
                    }
                }

                if ui.button("Store Original").clicked() {
                    if let Some(path) = &self.video_path {
                        let duration = self.video_info.as_ref().map(|info| info.duration_secs);
                        match self.library.import_original_video(path, duration) {
                            Ok(item) => {
                                self.library_status =
                                    Some(format!("Stored original: {}", item.title));
                            }
                            Err(e) => {
                                self.library_status = Some(format!("Store original failed: {}", e));
                            }
                        }
                    } else {
                        self.library_status = Some("Load a video before storing it".to_string());
                    }
                }

                if ui.button("Load ONNX Model").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ONNX Model", &["onnx"])
                        .pick_file()
                    {
                        self.model_path = Some(path);
                    }
                }

                if ui
                    .button(if self.live_mode.enabled {
                        "Stop Live Watch"
                    } else {
                        "Start Live Watch"
                    })
                    .clicked()
                {
                    self.toggle_live_capture();
                }

                ui.separator();

                // YouTube URL input
                let is_busy = matches!(
                    self.analysis_state,
                    AnalysisState::DownloadingVideo | AnalysisState::LoadingVideo
                );
                ui.label("URL:");
                let response = ui.add_sized(
                    [220.0, 18.0],
                    egui::TextEdit::singleline(&mut self.url_input)
                        .hint_text("YouTube / video URL")
                        .interactive(!is_busy),
                );
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let load_clicked = ui
                    .add_enabled(
                        !is_busy && !self.url_input.trim().is_empty(),
                        egui::Button::new("Load URL"),
                    )
                    .clicked();
                if (load_clicked || enter_pressed) && !is_busy && !self.url_input.trim().is_empty()
                {
                    let url = self.url_input.trim().to_string();
                    self.download_youtube(url);
                }

                ui.checkbox(&mut self.use_browser_cookies_for_url_loader, "Cookies");
                egui::ComboBox::from_id_salt("url_loader_cookie_browser")
                    .selected_text(self.browser_cookie_source.label())
                    .show_ui(ui, |ui| {
                        for source in [
                            BrowserCookieSource::Chrome,
                            BrowserCookieSource::Safari,
                            BrowserCookieSource::Firefox,
                        ] {
                            ui.selectable_value(
                                &mut self.browser_cookie_source,
                                source,
                                source.label(),
                            );
                        }
                    });

                ui.separator();

                // Status indicator
                let status_text = match &self.analysis_state {
                    AnalysisState::Idle => "Ready".to_string(),
                    AnalysisState::DownloadingVideo => "Downloading video...".to_string(),
                    AnalysisState::LoadingVideo => "Loading video...".to_string(),
                    AnalysisState::RunningDetection { progress } => {
                        format!("Detecting objects... {:.0}%", progress * 100.0)
                    }
                    AnalysisState::RunningTracker => "Running tracker...".to_string(),
                    AnalysisState::ComputingMetrics => "Computing metrics...".to_string(),
                    AnalysisState::GeneratingInsights => "Generating insights...".to_string(),
                    AnalysisState::Complete => "Analysis complete".to_string(),
                    AnalysisState::Error(e) => format!("Error: {}", e),
                };

                let status_color = match &self.analysis_state {
                    AnalysisState::Complete => colors::ACCENT,
                    AnalysisState::Error(_) => egui::Color32::RED,
                    AnalysisState::Idle => colors::TEXT_SECONDARY,
                    _ => egui::Color32::YELLOW,
                };

                ui.label(egui::RichText::new(status_text).color(status_color));
            });
        });

        // Bottom panel: timeline / scrubber
        egui::TopBottomPanel::bottom("timeline_panel")
            .min_height(60.0)
            .show(ctx, |ui| {
                let mut normalize_trim_after_ui = false;

                ui.horizontal(|ui| {
                    // Play/Pause button
                    let play_text = if self.is_playing { "Pause" } else { "Play" };
                    if ui.button(play_text).clicked() {
                        self.is_playing = !self.is_playing;
                        self.last_frame_time = ctx.input(|i| i.time);
                    }

                    // Frame scrubber
                    if !self.frames.is_empty() {
                        let mut frame_f = self.displayed_frame_position() as f32;
                        let max = (self.displayed_frame_count().saturating_sub(1)) as f32;
                        if ui
                            .add(
                                egui::Slider::new(&mut frame_f, 0.0..=max)
                                    .text(if self.playback_trimmed_only {
                                        "Trimmed Frame"
                                    } else {
                                        "Frame"
                                    })
                                    .show_value(true),
                            )
                            .changed()
                        {
                            self.set_displayed_frame_position(frame_f as usize);
                            self.is_playing = false;
                        }

                        let time = self.frames[self.current_frame_idx].timestamp_secs;
                        ui.label(format!("{:.2}s", time));

                        ui.separator();

                        if ui.button("Mark Start").clicked() {
                            self.analysis_start_idx = self.current_frame_idx;
                        }

                        if ui.button("Mark End").clicked() {
                            self.analysis_end_idx = self.current_frame_idx;
                        }

                        if ui.button("Reset Range").clicked() {
                            self.analysis_start_idx = 0;
                            self.analysis_end_idx = self.frames.len().saturating_sub(1);
                        }

                        if let Some((start, end)) = self.analysis_range() {
                            let start_time = self.frames[start].timestamp_secs;
                            let end_time = self.frames[end].timestamp_secs;
                            ui.label(format!(
                                "Analyze {:.2}s-{:.2}s ({} frames)",
                                start_time,
                                end_time,
                                end - start + 1
                            ));
                        }
                    }

                    ui.separator();

                    // Run Analysis button
                    let disabled_reason = if self.frames.is_empty() {
                        Some("Load a video first.")
                    } else if self.model_path.is_none() {
                        Some("Load an ONNX model first.")
                    } else if !self.homography_calibration.is_ready()
                        && !self.auto_pitch_ready
                        && self
                            .trim_suggestion
                            .as_ref()
                            .map_or(true, |trim| trim.segments.is_empty())
                    {
                        Some("Set 4 pitch landmarks, or use a frame where the pitch is visible.")
                    } else if !matches!(
                        self.analysis_state,
                        AnalysisState::Idle | AnalysisState::Complete | AnalysisState::Error(_)
                    ) {
                        Some("Wait for the current background task to finish.")
                    } else {
                        None
                    };
                    let can_analyze = disabled_reason.is_none();

                    if ui
                        .add_enabled(
                            can_analyze,
                            egui::Button::new(egui::RichText::new("Run Analysis").strong()),
                        )
                        .clicked()
                    {
                        self.run_analysis();
                    }

                    if let Some(reason) = disabled_reason {
                        ui.label(
                            egui::RichText::new(reason)
                                .color(colors::TEXT_SECONDARY)
                                .small(),
                        );
                    }

                    // Export button
                    if matches!(self.analysis_state, AnalysisState::Complete) {
                        if ui.button("Save Trim To Library").clicked() {
                            self.save_current_trim_to_library();
                        }

                        if ui.button("Export JSON").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .set_file_name("analysis_report.json")
                                .save_file()
                            {
                                if let (Some(tracking), Some(metrics)) =
                                    (&self.tracking, &self.metrics)
                                {
                                    let video_path = self
                                        .video_path
                                        .as_ref()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    if let Err(e) = crate::export::export_json(
                                        &path,
                                        &video_path,
                                        tracking,
                                        metrics,
                                        &self.insights,
                                    ) {
                                        log::error!("Export failed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                });

                if let Some(trim) = &mut self.trim_suggestion {
                    if !self.frames.is_empty() && !trim.segments.is_empty() {
                        ui.add_space(6.0);
                        let mut jump_to_segment = None;
                        let timeline_height = 26.0;
                        let available_width = ui.available_width().max(200.0);
                        let (response, painter) = ui.allocate_painter(
                            egui::vec2(available_width, timeline_height),
                            egui::Sense::click_and_drag(),
                        );
                        let rect = response.rect;
                        let total_frames = self.frames.len().max(1) as f32;

                        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(38));

                        for (segment_idx, segment) in trim.segments.iter_mut().enumerate() {
                            let x1 = rect.left()
                                + rect.width() * (segment.start_idx as f32 / total_frames);
                            let x2 = rect.left()
                                + rect.width()
                                    * (((segment.end_idx + 1) as f32 / total_frames).min(1.0));
                            let seg_rect = egui::Rect::from_min_max(
                                egui::pos2(x1.max(rect.left()), rect.top()),
                                egui::pos2(x2.max(x1 + 2.0).min(rect.right()), rect.bottom()),
                            );

                            let fill = if segment.enabled {
                                colors::ACCENT
                            } else {
                                egui::Color32::from_gray(80)
                            };
                            painter.rect_filled(seg_rect, 3.0, fill.gamma_multiply(0.75));
                            painter.rect_stroke(
                                seg_rect,
                                3.0,
                                egui::Stroke::new(1.0, fill),
                                egui::StrokeKind::Outside,
                            );

                            let start_handle = egui::Rect::from_center_size(
                                egui::pos2(seg_rect.left(), seg_rect.center().y),
                                egui::vec2(8.0, timeline_height),
                            );
                            let end_handle = egui::Rect::from_center_size(
                                egui::pos2(seg_rect.right(), seg_rect.center().y),
                                egui::vec2(8.0, timeline_height),
                            );

                            let start_id = ui.id().with(("trim_start", segment_idx));
                            let end_id = ui.id().with(("trim_end", segment_idx));
                            let body_id = ui.id().with(("trim_body", segment_idx));

                            let start_resp =
                                ui.interact(start_handle, start_id, egui::Sense::drag());
                            let end_resp = ui.interact(end_handle, end_id, egui::Sense::drag());
                            let body_resp = ui.interact(seg_rect, body_id, egui::Sense::click());

                            if start_resp.dragged() {
                                if let Some(pointer) = start_resp.interact_pointer_pos() {
                                    let ratio =
                                        ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                    let new_start = (ratio
                                        * (self.frames.len().saturating_sub(1)) as f32)
                                        .round()
                                        as usize;
                                    segment.start_idx = new_start.min(segment.end_idx);
                                    normalize_trim_after_ui = true;
                                }
                            }

                            if end_resp.dragged() {
                                if let Some(pointer) = end_resp.interact_pointer_pos() {
                                    let ratio =
                                        ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                    let new_end = (ratio
                                        * (self.frames.len().saturating_sub(1)) as f32)
                                        .round()
                                        as usize;
                                    segment.end_idx = new_end.max(segment.start_idx);
                                    normalize_trim_after_ui = true;
                                }
                            }

                            if body_resp.clicked() {
                                jump_to_segment = Some(segment_idx);
                            }

                            painter.line_segment(
                                [
                                    egui::pos2(seg_rect.left(), seg_rect.top()),
                                    egui::pos2(seg_rect.left(), seg_rect.bottom()),
                                ],
                                egui::Stroke::new(2.0, egui::Color32::WHITE),
                            );
                            painter.line_segment(
                                [
                                    egui::pos2(seg_rect.right(), seg_rect.top()),
                                    egui::pos2(seg_rect.right(), seg_rect.bottom()),
                                ],
                                egui::Stroke::new(2.0, egui::Color32::WHITE),
                            );
                        }

                        let playhead_ratio = self.current_frame_idx as f32 / total_frames;
                        let playhead_x = rect.left() + rect.width() * playhead_ratio;
                        painter.line_segment(
                            [
                                egui::pos2(playhead_x, rect.top()),
                                egui::pos2(playhead_x, rect.bottom()),
                            ],
                            egui::Stroke::new(2.0, egui::Color32::YELLOW),
                        );

                        if let Some(segment_idx) = jump_to_segment {
                            self.jump_to_segment(segment_idx);
                        }
                    }
                }

                if normalize_trim_after_ui {
                    self.normalize_trim_segments();
                }
            });

        // Right panel: Analysis / Controls
        egui::SidePanel::right("analysis_panel")
            .min_width(360.0)
            .default_width(420.0)
            .show(ctx, |ui| {
                super::analysis_panel::show(self, ui);
            });

        // Central panel: Video view
        egui::CentralPanel::default().show(ctx, |ui| {
            super::video_panel::show(self, ui, ctx);
        });
    }
}
