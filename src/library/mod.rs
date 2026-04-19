// library/mod.rs — Local media library for original videos and trimmed clips

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaKind {
    Original,
    Trimmed,
}

impl std::fmt::Display for MediaKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaKind::Original => write!(f, "Original"),
            MediaKind::Trimmed => write!(f, "Trimmed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub title: String,
    pub kind: MediaKind,
    pub file_path: PathBuf,
    pub source_video_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub duration_secs: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryDb {
    pub items: Vec<LibraryItem>,
}

#[derive(Debug, Clone)]
pub struct MediaLibrary {
    root_dir: PathBuf,
    db_path: PathBuf,
    pub db: LibraryDb,
}

impl MediaLibrary {
    pub fn load_or_create() -> Result<Self> {
        let root_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("football-coach-poc");
        let originals_dir = root_dir.join("originals");
        let trims_dir = root_dir.join("trimmed");
        fs::create_dir_all(&originals_dir)?;
        fs::create_dir_all(&trims_dir)?;

        let db_path = root_dir.join("library.json");
        let db = if db_path.exists() {
            let contents = fs::read_to_string(&db_path)?;
            serde_json::from_str(&contents).unwrap_or_default()
        } else {
            LibraryDb::default()
        };

        Ok(Self {
            root_dir,
            db_path,
            db,
        })
    }

    pub fn items(&self) -> &[LibraryItem] {
        &self.db.items
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.db)?;
        fs::write(&self.db_path, json)?;
        Ok(())
    }

    pub fn import_original_video(
        &mut self,
        source_path: &Path,
        duration_secs: Option<f64>,
    ) -> Result<LibraryItem> {
        let stored_path = self.copy_into("originals", source_path)?;
        let item = LibraryItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: source_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            kind: MediaKind::Original,
            file_path: stored_path,
            source_video_path: None,
            created_at: Utc::now(),
            duration_secs,
            notes: None,
        };
        self.upsert_item(item.clone())?;
        Ok(item)
    }

    pub fn register_existing_original(
        &mut self,
        source_path: &Path,
        duration_secs: Option<f64>,
    ) -> Result<LibraryItem> {
        if let Some(existing) = self
            .db
            .items
            .iter()
            .find(|item| item.kind == MediaKind::Original && item.file_path == source_path)
        {
            return Ok(existing.clone());
        }

        let item = LibraryItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: source_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            kind: MediaKind::Original,
            file_path: source_path.to_path_buf(),
            source_video_path: None,
            created_at: Utc::now(),
            duration_secs,
            notes: Some("Referenced from current workspace".to_string()),
        };
        self.upsert_item(item.clone())?;
        Ok(item)
    }

    pub fn save_trimmed_clip(
        &mut self,
        source_video_path: &Path,
        trim_title: &str,
        content: &str,
        segments: &[(f64, f64)],
        duration_secs: Option<f64>,
    ) -> Result<LibraryItem> {
        let file_name = sanitize_file_name(trim_title);
        let trim_id = uuid::Uuid::new_v4().to_string();
        let stored_path = self
            .root_dir
            .join("trimmed")
            .join(format!("{}-{}.mp4", file_name, trim_id));
        let manifest_path = self
            .root_dir
            .join("trimmed")
            .join(format!("{}-{}.txt", file_name, trim_id));

        render_trimmed_video(source_video_path, &stored_path, segments)?;
        fs::write(&manifest_path, content).context("Failed to write trimmed clip manifest")?;

        let item = LibraryItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: trim_title.to_string(),
            kind: MediaKind::Trimmed,
            file_path: stored_path,
            source_video_path: Some(source_video_path.to_path_buf()),
            created_at: Utc::now(),
            duration_secs,
            notes: Some(format!("Trim manifest: {}", manifest_path.display())),
        };
        self.upsert_item(item.clone())?;
        Ok(item)
    }

    fn upsert_item(&mut self, item: LibraryItem) -> Result<()> {
        self.db.items.retain(|existing| existing.id != item.id);
        self.db.items.push(item);
        self.db
            .items
            .sort_by(|a, b| b.created_at.cmp(&a.created_at));
        self.save()
    }

    fn copy_into(&self, subdir: &str, source_path: &Path) -> Result<PathBuf> {
        let ext = source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("bin");
        let file_name = format!(
            "{}-{}.{}",
            sanitize_file_name(
                &source_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
            uuid::Uuid::new_v4(),
            ext
        );
        let target = self.root_dir.join(subdir).join(file_name);
        fs::copy(source_path, &target).with_context(|| {
            format!(
                "Failed to copy media into library: {} -> {}",
                source_path.display(),
                target.display()
            )
        })?;
        Ok(target)
    }
}

fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    cleaned.trim_matches('-').to_lowercase()
}

fn render_trimmed_video(
    source_video_path: &Path,
    output_path: &Path,
    segments: &[(f64, f64)],
) -> Result<()> {
    let parent = output_path
        .parent()
        .context("Missing trimmed output parent dir")?;
    let temp_dir = parent.join(format!("tmp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)?;

    let concat_list_path = temp_dir.join("concat.txt");
    let mut concat_list = String::new();

    for (idx, (start, end)) in segments.iter().enumerate() {
        let duration = (end - start).max(0.0);
        if duration <= 0.05 {
            continue;
        }

        let chunk_path = temp_dir.join(format!("segment-{:03}.ts", idx));
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{start:.3}"))
            .arg("-i")
            .arg(source_video_path)
            .arg("-t")
            .arg(format!("{duration:.3}"))
            .arg("-c")
            .arg("copy")
            .arg("-bsf:v")
            .arg("h264_mp4toannexb")
            .arg("-f")
            .arg("mpegts")
            .arg(&chunk_path)
            .status()
            .with_context(|| format!("Failed to invoke ffmpeg for segment {}", idx + 1))?;

        if !status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg failed while rendering trimmed segment {}",
                idx + 1
            ));
        }

        concat_list.push_str(&format!("file '{}'\n", chunk_path.display()));
    }

    if concat_list.is_empty() {
        return Err(anyhow::anyhow!("No valid trim segments to render"));
    }

    fs::write(&concat_list_path, concat_list)?;

    let concat_status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&concat_list_path)
        .arg("-c")
        .arg("copy")
        .arg("-bsf:a")
        .arg("aac_adtstoasc")
        .arg(output_path)
        .status()
        .context("Failed to invoke ffmpeg concat")?;

    if !concat_status.success() {
        return Err(anyhow::anyhow!(
            "ffmpeg failed while concatenating trimmed clip"
        ));
    }

    let _ = fs::remove_dir_all(&temp_dir);
    Ok(())
}
