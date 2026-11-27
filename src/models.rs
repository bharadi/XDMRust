use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileCategory {
    All,
    Video,
    Audio,
    Compressed,
    Document,
    Program,
    Other,
}

impl FileCategory {
    pub fn from_filename(name: &str) -> Self {
        let ext = name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" => FileCategory::Video,
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => FileCategory::Audio,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" => FileCategory::Compressed,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "csv" => {
                FileCategory::Document
            }
            "exe" | "msi" | "bat" | "cmd" | "apk" | "dmg" | "deb" | "rpm" => FileCategory::Program,
            _ => FileCategory::Other,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            FileCategory::All => "All Downloads",
            FileCategory::Video => "Videos",
            FileCategory::Audio => "Music & Audio",
            FileCategory::Compressed => "Compressed / Archives",
            FileCategory::Document => "Documents",
            FileCategory::Program => "Programs & Installers",
            FileCategory::Other => "Others",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            FileCategory::All => "📦",
            FileCategory::Video => "🎬",
            FileCategory::Audio => "🎵",
            FileCategory::Compressed => "🗜️",
            FileCategory::Document => "📄",
            FileCategory::Program => "⚙️",
            FileCategory::Other => "📁",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Stopped,
    Completed,
    Error(String),
}

impl DownloadStatus {
    pub fn display(&self) -> String {
        match self {
            DownloadStatus::Queued => "Queued".to_string(),
            DownloadStatus::Connecting => "Connecting...".to_string(),
            DownloadStatus::Downloading => "Downloading".to_string(),
            DownloadStatus::Paused => "Paused".to_string(),
            DownloadStatus::Stopped => "Stopped".to_string(),
            DownloadStatus::Completed => "Completed".to_string(),
            DownloadStatus::Error(msg) => format!("Error: {}", msg),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, DownloadStatus::Downloading | DownloadStatus::Connecting)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkProgress {
    pub id: usize,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub is_finished: bool,
}

impl ChunkProgress {
    pub fn total_bytes(&self) -> u64 {
        if self.end_byte >= self.start_byte {
            self.end_byte - self.start_byte + 1
        } else {
            0
        }
    }

    pub fn fraction(&self) -> f32 {
        let total = self.total_bytes();
        if total == 0 {
            0.0
        } else {
            (self.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: f64,
    pub status: DownloadStatus,
    pub category: FileCategory,
    pub supports_ranges: bool,
    pub num_connections: usize,
    pub chunks: Vec<ChunkProgress>,
    pub date_added: DateTime<Utc>,
    pub eta_seconds: Option<u64>,
}

impl DownloadItem {
    pub fn progress_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            if matches!(self.status, DownloadStatus::Completed) {
                1.0
            } else {
                0.0
            }
        } else {
            (self.downloaded_bytes as f32 / self.total_bytes as f32).clamp(0.0, 1.0)
        }
    }

    pub fn formatted_size(&self) -> String {
        if self.total_bytes == 0 {
            format_bytes(self.downloaded_bytes)
        } else {
            format!(
                "{} / {}",
                format_bytes(self.downloaded_bytes),
                format_bytes(self.total_bytes)
            )
        }
    }

    pub fn formatted_speed(&self) -> String {
        if self.status.is_active() && self.speed_bytes_per_sec > 0.0 {
            format!("{}/s", format_bytes(self.speed_bytes_per_sec as u64))
        } else {
            "-".to_string()
        }
    }

    pub fn formatted_eta(&self) -> String {
        if let Some(sec) = self.eta_seconds {
            if self.status.is_active() && sec > 0 {
                let m = sec / 60;
                let s = sec % 60;
                let h = m / 60;
                if h > 0 {
                    format!("{}h {}m", h, m % 60)
                } else if m > 0 {
                    format!("{}m {}s", m, s)
                } else {
                    format!("{}s", s)
                }
            } else {
                "-".to_string()
            }
        } else {
            "-".to_string()
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[derive(Debug, Clone)]
pub enum DownloadEvent {
    StatusChanged(String, DownloadStatus),
    MetadataProbed {
        id: String,
        total_bytes: u64,
        supports_ranges: bool,
        filename: String,
    },
    ProgressUpdated {
        id: String,
        downloaded_bytes: u64,
        total_bytes: u64,
        speed_bytes_per_sec: f64,
        eta_seconds: Option<u64>,
        chunks: Vec<ChunkProgress>,
    },
    Cancelled(String),
    Completed(String),
}
