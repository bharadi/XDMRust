use crate::models::{DownloadEvent, DownloadItem, DownloadStatus, FileCategory};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_download_dir: PathBuf,
    pub max_active_downloads: usize,
    pub default_connections: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        let download_dir = dirs_downloads().unwrap_or_else(|| PathBuf::from("Downloads"));
        Self {
            default_download_dir: download_dir,
            max_active_downloads: 3,
            default_connections: 8,
        }
    }
}

pub struct AppState {
    pub items: Vec<DownloadItem>,
    pub selected_id: Option<String>,
    pub active_category: FileCategory,
    pub config: AppConfig,
    pub active_cancels: HashMap<String, Arc<AtomicBool>>,
}

impl AppState {
    pub fn new() -> Self {
        let config = AppConfig::default();
        let items = load_persisted_items().unwrap_or_default();

        Self {
            items,
            selected_id: None,
            active_category: FileCategory::All,
            config,
            active_cancels: HashMap::new(),
        }
    }

    pub fn save_items(&self) {
        let _ = save_persisted_items(&self.items);
    }

    pub fn add_item(&mut self, item: DownloadItem) {
        self.items.insert(0, item);
        self.save_items();
    }

    pub fn remove_item(&mut self, id: &str, delete_file: bool) {
        if let Some(pos) = self.items.iter().position(|it| it.id == id) {
            let item = self.items.remove(pos);
            if let Some(cancel) = self.active_cancels.remove(id) {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if delete_file {
                let _ = fs::remove_file(&item.save_path);
            }
            let _ = fs::remove_dir_all(
                item.save_path
                    .parent()
                    .unwrap_or(&PathBuf::from("."))
                    .join(format!(".xdm_parts_{}", id)),
            );
            self.save_items();
        }
    }

    pub fn handle_event(&mut self, event: DownloadEvent) {
        match event {
            DownloadEvent::StatusChanged(id, status) => {
                if let Some(item) = self.items.iter_mut().find(|it| it.id == id) {
                    item.status = status;
                }
                self.save_items();
            }
            DownloadEvent::MetadataProbed {
                id,
                total_bytes,
                supports_ranges,
                filename,
            } => {
                if let Some(item) = self.items.iter_mut().find(|it| it.id == id) {
                    if total_bytes > 0 {
                        item.total_bytes = total_bytes;
                    }
                    item.supports_ranges = supports_ranges;

                    let should_update_name = !filename.is_empty() && filename != "download.bin" && (
                        item.filename == "download.bin"
                        || item.filename.is_empty()
                        || item.filename.starts_with("videoplayback")
                        || item.filename.starts_with("stream")
                        || item.filename.starts_with("blob")
                        || (!item.filename.contains('.') && filename.contains('.'))
                    );

                    if should_update_name {
                        let parent = item.save_path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
                        item.filename = filename;
                        item.save_path = parent.join(&item.filename);
                        item.category = FileCategory::from_filename(&item.filename);
                    }
                }
                self.save_items();
            }
            DownloadEvent::ProgressUpdated {
                id,
                downloaded_bytes,
                total_bytes,
                speed_bytes_per_sec,
                eta_seconds,
                chunks,
            } => {
                if let Some(item) = self.items.iter_mut().find(|it| it.id == id) {
                    item.downloaded_bytes = downloaded_bytes;
                    if total_bytes > 0 && item.total_bytes == 0 {
                        item.total_bytes = total_bytes;
                    }
                    item.speed_bytes_per_sec = speed_bytes_per_sec;
                    item.eta_seconds = eta_seconds;
                    item.chunks = chunks;
                }
            }
            DownloadEvent::Cancelled(id) => {
                if let Some(item) = self.items.iter_mut().find(|it| it.id == id) {
                    if item.status.is_active() {
                        item.status = DownloadStatus::Paused;
                    }
                    item.speed_bytes_per_sec = 0.0;
                    item.eta_seconds = None;
                }
                self.active_cancels.remove(&id);
                self.save_items();
            }
            DownloadEvent::Completed(id) => {
                if let Some(item) = self.items.iter_mut().find(|it| it.id == id) {
                    item.status = DownloadStatus::Completed;
                    item.speed_bytes_per_sec = 0.0;
                    item.eta_seconds = None;
                    if item.total_bytes > 0 {
                        item.downloaded_bytes = item.total_bytes;
                    }
                }
                self.active_cancels.remove(&id);
                self.save_items();
            }
        }
    }
}

fn config_path() -> PathBuf {
    let mut base = dirs_data().unwrap_or_else(|| PathBuf::from("."));
    base.push("XDM-Rust");
    let _ = fs::create_dir_all(&base);
    base.join("downloads_state.json")
}

fn load_persisted_items() -> Option<Vec<DownloadItem>> {
    let path = config_path();
    let content = fs::read_to_string(path).ok()?;
    let mut items: Vec<DownloadItem> = serde_json::from_str(&content).ok()?;
    for item in items.iter_mut() {
        if item.status.is_active() {
            item.status = DownloadStatus::Paused;
            item.speed_bytes_per_sec = 0.0;
            item.eta_seconds = None;
        }
    }
    Some(items)
}

fn save_persisted_items(items: &[DownloadItem]) -> std::io::Result<()> {
    let path = config_path();
    let json = serde_json::to_string_pretty(items)?;
    fs::write(path, json)
}

fn dirs_downloads() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|prof| PathBuf::from(prof).join("Downloads"))
}

fn dirs_data() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}
