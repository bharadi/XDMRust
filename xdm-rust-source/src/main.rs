#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod models;
mod state;
mod tray;
mod ui;

use chrono::Utc;
use eframe::egui::{self, Color32, Vec2};
use engine::DownloadEngine;
use models::{DownloadEvent, DownloadItem, DownloadStatus, FileCategory};
use state::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tray::{TrayEvent, TrayManager};
use ui::{AboutDialog, NewDownloadDialog, RenameDialog, UiAction};

struct XdmApp {
    state: AppState,
    dialog: NewDownloadDialog,
    rename_dialog: RenameDialog,
    about_dialog: AboutDialog,
    engine: Arc<DownloadEngine>,
    event_rx: UnboundedReceiver<DownloadEvent>,
    _tray: TrayManager,
    tray_rx: UnboundedReceiver<TrayEvent>,
    tokio_handle: tokio::runtime::Handle,
    is_hidden_to_tray: bool,
}

impl XdmApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        engine: Arc<DownloadEngine>,
        event_rx: UnboundedReceiver<DownloadEvent>,
        tray: TrayManager,
        tray_rx: UnboundedReceiver<TrayEvent>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        // Register egui context with tray manager so tray clicks can wake up the GUI thread immediately
        tray::set_egui_context(_cc.egui_ctx.clone());

        Self {
            state: AppState::new(),
            dialog: NewDownloadDialog::default(),
            rename_dialog: RenameDialog::default(),
            about_dialog: AboutDialog::default(),
            engine,
            event_rx,
            _tray: tray,
            tray_rx,
            tokio_handle,
            is_hidden_to_tray: false,
        }
    }

    fn handle_actions(&mut self, actions: Vec<UiAction>) {
        for action in actions {
            match action {
                UiAction::StartNewDownload {
                    url,
                    filename,
                    destination,
                    connections,
                    total_bytes,
                    supports_ranges,
                } => {
                    let id = uuid::Uuid::new_v4().to_string();
                    let category = FileCategory::from_filename(&filename);

                    let item = DownloadItem {
                        id: id.clone(),
                        url: url.clone(),
                        filename,
                        save_path: destination,
                        total_bytes,
                        downloaded_bytes: 0,
                        speed_bytes_per_sec: 0.0,
                        status: DownloadStatus::Queued,
                        category,
                        supports_ranges,
                        num_connections: connections,
                        chunks: Vec::new(),
                        date_added: Utc::now(),
                        eta_seconds: None,
                    };

                    self.state.add_item(item.clone());
                    self.start_download_task(item);
                }

                UiAction::PauseDownload(id) => {
                    if let Some(cancel) = self.state.active_cancels.remove(&id) {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Some(item) = self.state.items.iter_mut().find(|i| i.id == id) {
                        item.status = DownloadStatus::Paused;
                        item.speed_bytes_per_sec = 0.0;
                        item.eta_seconds = None;
                    }
                    self.state.save_items();
                }

                UiAction::StopDownload(id) => {
                    if let Some(cancel) = self.state.active_cancels.remove(&id) {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Some(item) = self.state.items.iter_mut().find(|i| i.id == id) {
                        item.status = DownloadStatus::Stopped;
                        item.speed_bytes_per_sec = 0.0;
                        item.eta_seconds = None;
                    }
                    self.state.save_items();
                }

                UiAction::ResumeDownload(id) => {
                    if let Some(item) = self.state.items.iter_mut().find(|i| i.id == id) {
                        if !item.status.is_active() && item.status != DownloadStatus::Completed {
                            item.status = DownloadStatus::Connecting;
                            let item_clone = item.clone();
                            self.start_download_task(item_clone);
                        }
                    }
                }

                UiAction::DeleteDownload { id, delete_file } => {
                    self.state.remove_item(&id, delete_file);
                }

                UiAction::OpenDownloadedFile(path) => {
                    let _ = open::that_detached(&path);
                }

                UiAction::OpenContainingFolder(path) => {
                    let folder = if path.is_file() || path.extension().is_some() {
                        path.parent().map(|p| p.to_path_buf())
                    } else {
                        Some(path)
                    };
                    if let Some(dir) = folder {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = open::that_detached(&dir);
                    }
                }

                UiAction::OpenRenameDialog { id, current_name } => {
                    self.rename_dialog.is_open = true;
                    self.rename_dialog.item_id = id;
                    self.rename_dialog.new_name = current_name;
                }

                UiAction::RenameDownload { id, new_filename } => {
                    let clean_name = crate::engine::sanitize_filename(&new_filename);
                    if !clean_name.is_empty() {
                        if let Some(item) = self.state.items.iter_mut().find(|i| i.id == id) {
                            let old_path = item.save_path.clone();
                            let parent = old_path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
                            let new_path = parent.join(&clean_name);

                            if item.status == DownloadStatus::Completed && old_path.exists() && old_path != new_path {
                                let _ = std::fs::rename(&old_path, &new_path);
                            }

                            item.filename = clean_name.clone();
                            item.save_path = new_path;
                            item.category = FileCategory::from_filename(&clean_name);
                        }
                        self.state.save_items();
                    }
                }

                UiAction::OpenAboutDialog => {
                    self.about_dialog.is_open = true;
                }

                UiAction::MinimizeToTray => {
                    self.is_hidden_to_tray = true;
                    // Hide directly via Win32 so Windows hides the taskbar button immediately
                    unsafe {
                        let title = tray::to_wide(tray::APP_TITLE);
                        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(std::ptr::null(), title.as_ptr());
                        if hwnd != 0 {
                            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE);
                        }
                    }
                }
            }
        }
    }

    fn start_download_task(&mut self, item: DownloadItem) {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.state
            .active_cancels
            .insert(item.id.clone(), cancel_flag.clone());

        let engine = self.engine.clone();
        let handle = self.tokio_handle.clone();

        handle.spawn(async move {
            engine.execute_download(item, cancel_flag).await;
        });
    }
}

impl eframe::App for XdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle incoming system tray events from next to the clock
        while let Ok(tray_event) = self.tray_rx.try_recv() {
            match tray_event {
                TrayEvent::RestoreWindow => {
                    self.is_hidden_to_tray = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
                TrayEvent::PauseAll => {
                    let active_ids: Vec<String> = self
                        .state
                        .items
                        .iter()
                        .filter(|i| i.status.is_active())
                        .map(|i| i.id.clone())
                        .collect();
                    for id in active_ids {
                        self.handle_actions(vec![UiAction::PauseDownload(id)]);
                    }
                }
                TrayEvent::ResumeAll => {
                    let resumable_ids: Vec<String> = self
                        .state
                        .items
                        .iter()
                        .filter(|i| !i.status.is_active() && i.status != DownloadStatus::Completed)
                        .map(|i| i.id.clone())
                        .collect();
                    for id in resumable_ids {
                        self.handle_actions(vec![UiAction::ResumeDownload(id)]);
                    }
                }
                TrayEvent::ExitApp => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // When window is minimized by the user, hide it from the taskbar so it docks into tray
        let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if is_minimized && !self.is_hidden_to_tray {
            self.is_hidden_to_tray = true;
            unsafe {
                let title = tray::to_wide(tray::APP_TITLE);
                let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(std::ptr::null(), title.as_ptr());
                if hwnd != 0 {
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE);
                }
            }
        } else if !is_minimized && self.is_hidden_to_tray {
            self.is_hidden_to_tray = false;
        }

        // Drain incoming background download events
        let mut got_event = false;
        while let Ok(event) = self.event_rx.try_recv() {
            self.state.handle_event(event);
            got_event = true;
        }

        // Request continuous repaint if any download is active or if we just received updates
        let has_active = self.state.items.iter().any(|i| i.status.is_active());
        if has_active || got_event {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let mut actions = Vec::new();

        // Top Command Bar
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(18, 20, 24))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui::render_top_bar(ui, &self.state, &mut self.dialog, &mut actions);
            });

        // Left Navigation Sidebar
        egui::SidePanel::left("left_sidebar")
            .resizable(false)
            .default_width(180.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(14, 16, 20))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui::render_sidebar(ui, &mut self.state, &mut actions);
            });

        // Bottom Details / Chunk Visualizer Panel (if an item is selected)
        if let Some(selected_id) = &self.state.selected_id {
            if let Some(item) = self.state.items.iter().find(|i| &i.id == selected_id) {
                if !item.chunks.is_empty() {
                    egui::TopBottomPanel::bottom("bottom_chunks_panel")
                        .resizable(true)
                        .default_height(110.0)
                        .frame(
                            egui::Frame::none()
                                .fill(Color32::from_rgb(22, 24, 30))
                                .inner_margin(8.0),
                        )
                        .show(ctx, |ui| {
                            ui::render_chunk_visualizer(ui, item);
                        });
                }
            }
        }

        // Central Queue List
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(11, 13, 17))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui::render_download_table(ui, &mut self.state, &mut actions);
            });

        // "Add Download" Modal Dialog
        ui::render_new_download_modal(ctx, &mut self.dialog, &mut actions);

        // "Rename File" Modal Dialog
        ui::render_rename_modal(ctx, &mut self.rename_dialog, &mut actions);

        // "About XDM-Rust & Privacy Policy" Modal Dialog
        ui::render_about_modal(ctx, &mut self.about_dialog);

        // Process any queued user actions
        self.handle_actions(actions);
    }
}

fn main() -> eframe::Result<()> {
    // Start background Tokio multi-threaded runtime
    let rt = tokio::runtime::Runtime::new().expect("Failed to initialize Tokio runtime");
    let handle = rt.handle().clone();

    let (event_tx, event_rx) = unbounded_channel();
    let engine = Arc::new(DownloadEngine::new(event_tx));

    // Start Windows native System Tray next to clock
    let (tray, tray_rx) = TrayManager::start();

    // Embedded 64x64 raw RGBA application window icon
    let icon_bytes = include_bytes!("../app_icon_64.rgba");
    let icon = egui::IconData {
        rgba: icon_bytes.to_vec(),
        width: 64,
        height: 64,
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(980.0, 620.0))
            .with_min_inner_size(Vec2::new(760.0, 480.0))
            .with_title(tray::APP_TITLE)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };

    eframe::run_native(
        tray::APP_TITLE,
        native_options,
        Box::new(move |cc| {
            // Apply Modern Dark Palette
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = Color32::from_rgb(18, 20, 24);
            visuals.panel_fill = Color32::from_rgb(14, 16, 20);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(25, 28, 35);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(32, 36, 45);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 52, 65);
            visuals.widgets.active.bg_fill = Color32::from_rgb(0, 120, 215);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(XdmApp::new(cc, engine, event_rx, tray, tray_rx, handle)))
        }),
    )
}
