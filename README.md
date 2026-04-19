# Football Coach PoC — Tactical Analysis Desktop App

A Rust-based desktop application for football (soccer) tactical analysis. Load a match clip, detect players and the ball via YOLO, track them across frames, and get coach-friendly insights including heatmaps, movement metrics, and pressing analysis.

## Features (PoC)

- **Video Playback** — Load MP4/AVI/MKV clips, play/pause, frame-by-frame scrubbing
- **Object Detection** — YOLOv8/v11 via ONNX Runtime (players + ball)
- **Multi-Object Tracking** — ByteTrack-style tracker with persistent IDs
- **Pitch Homography** — Manual 4-corner calibration for 2D top-down mapping
- **Heatmaps** — Team and per-player occupancy on a pitch diagram
- **Movement Metrics** — Distance covered, average/max speed per player
- **Possession** — Proximity-based ball touch detection
- **Pressing Analysis** — Pressing intensity by pitch zone
- **Tactical Insights** — Rule-based text summaries for coaches
- **Export** — JSON data export with full tracking + metrics
- **Local Media Library** — Store original videos and render trimmed `.mp4` clips into a local app library

## Prerequisites

### 1. Rust Toolchain
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. FFmpeg Development Libraries

**macOS:**
```bash
brew install ffmpeg pkg-config
```

**Ubuntu/Debian:**
```bash
sudo apt install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev pkg-config
```

**Windows:**
Download FFmpeg shared libraries from https://github.com/BtbN/FFmpeg-Builds/releases
and set `FFMPEG_DIR` environment variable to the extracted path.

### 3. ONNX Runtime

The app now uses `ort` with bundled prebuilt binaries, so you do not need to manually install or point `ORT_DYLIB_PATH` at a shared library for normal builds.

The first build may download an ONNX Runtime package automatically during `cargo build`.

### 4. YOLO ONNX Model

A pre-exported `yolov8n.onnx` model is included in the `models/` directory. To use a different model, export via:

```bash
pip install ultralytics
yolo export model=yolov8n.pt format=onnx imgsz=640
```

**Recommended models for PoC:**
- `yolov8n.onnx` — Nano (fastest, ~6MB, good for testing)
- `yolov8s.onnx` — Small (~22MB, better accuracy)
- `yolov11n.onnx` — Latest architecture

## Build & Run

```bash
cd football-coach-poc

# Debug build (faster compile)
cargo run

# Release build (much faster inference)
cargo run --release
```

## Usage

1. **Load Video** — Click "Load Video" in the top bar, select an MP4 clip (30-90s recommended)
2. **Load Model** — Click "Load ONNX Model", select `models/yolov8n.onnx` (or your own .onnx file)
3. **Calibrate Pitch** — In Settings, check "Edit pitch corners" and click the 4 pitch corners on the video (TL, TR, BR, BL)
4. **Run Analysis** — Click the "Run Analysis" button in the bottom bar
5. **Review Results** — Use the tabs on the right: Insights, Tracking, Heatmaps, Pitch View, Report
6. **Store Media** — Click "Store Original" to copy the source video into the app library, or "Save Trim To Library" after analysis to render the selected segments as a trimmed `.mp4`
7. **Export** — Click "Export JSON" to save the full analysis data

## Project Structure

```
football-coach-poc/
  src/
    main.rs                 # Entry point
    video_processor/        # FFmpeg video decoding
      mod.rs
      decoder.rs
    detection/              # YOLO object detection
      mod.rs
      yolo.rs
    tracker/                # Multi-object tracking
      mod.rs
      bytetrack.rs
    pitch_mapping/          # Homography / 2D projection
      mod.rs
    metrics/                # Analytics computation
      mod.rs
      heatmap.rs
      movement.rs
      possession.rs
      pressing.rs
    tactical_insights/      # Coach-friendly text generation
      mod.rs
    gui/                    # egui/eframe UI
      mod.rs
      app.rs
      video_panel.rs
      analysis_panel.rs
      heatmap_view.rs
      pitch_overlay.rs
    export/                 # JSON/report export
      mod.rs
  models/                   # Place .onnx models here
  assets/                   # UI assets
  Cargo.toml
```

## Architecture

```
Video File
    |
    v
[ffmpeg-next decoder] --> Raw RGB Frames
    |
    v
[YOLOv8 ONNX (ort)] --> Per-frame Detections (BBox, class, confidence)
    |
    v
[ByteTrack Tracker]   --> Persistent Track IDs across frames
    |
    v
[Homography Mapper]   --> 2D pitch coordinates (meters)
    |
    v
[Metrics Engine]      --> Heatmaps, distances, speeds, possession, pressing
    |
    v
[Tactical Insights]   --> Coach-friendly text summaries
    |
    v
[egui GUI]            --> Video + overlays + analysis panels
```

## Known Limitations (PoC)

- **No team classification** — Players are not split into teams (would need jersey color clustering)
- **Single camera** — Assumes a single broadcast-angle camera
- **Manual homography** — Pitch corners must be set manually
- **Simplified tracking** — Greedy matching, no Re-ID for occluded players
- **No audio analysis** — Whistle/crowd analysis not implemented
- **Ball detection** — Small ball is hard to detect consistently with YOLO nano

## Future Improvements

- [ ] Automatic pitch line detection for homography
- [ ] Jersey color clustering for team assignment
- [ ] Pass detection (direction + speed of ball between players)
- [ ] Formation detection (convex hull / template matching)
- [ ] Local LLM integration (Ollama) for natural language tactical summaries
- [ ] Multi-camera support
- [ ] Real-time processing mode
- [ ] GPU acceleration for ONNX inference

## License

MIT
