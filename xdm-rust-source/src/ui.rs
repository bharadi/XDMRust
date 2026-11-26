use crate::models::{format_bytes, DownloadItem, DownloadStatus, FileCategory};
use crate::state::AppState;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use std::path::PathBuf;

pub struct NewDownloadDialog {
    pub is_open: bool,
    pub url: String,
    pub filename: String,
    pub destination_dir: PathBuf,
    pub connections: usize,
    pub probed_size: Option<u64>,
    pub supports_ranges: bool,
    pub error_msg: Option<String>,
}

impl Default for NewDownloadDialog {
    fn default() -> Self {
        let default_dir = std::env::var_os("USERPROFILE")
            .map(|p| PathBuf::from(p).join("Downloads"))
            .unwrap_or_else(|| PathBuf::from("Downloads"));

        Self {
            is_open: false,
            url: String::new(),
            filename: String::new(),
            destination_dir: default_dir,
            connections: 8,
            probed_size: None,
            supports_ranges: true,
            error_msg: None,
        }
    }
}

#[derive(Default)]
pub struct RenameDialog {
    pub is_open: bool,
    pub item_id: String,
    pub new_name: String,
}

#[derive(Default, PartialEq, Eq)]
pub enum AboutTab {
    #[default]
    TermsOfUse,
    PrivacyPolicy,
}

pub struct AboutDialog {
    pub is_open: bool,
    pub active_tab: AboutTab,
}

impl Default for AboutDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: AboutTab::TermsOfUse,
        }
    }
}

pub enum UiAction {
    StartNewDownload {
        url: String,
        filename: String,
        destination: PathBuf,
        connections: usize,
        total_bytes: u64,
        supports_ranges: bool,
    },
    PauseDownload(String),
    ResumeDownload(String),
    StopDownload(String),
    DeleteDownload {
        id: String,
        delete_file: bool,
    },
    OpenDownloadedFile(PathBuf),
    OpenContainingFolder(PathBuf),
    OpenRenameDialog {
        id: String,
        current_name: String,
    },
    RenameDownload {
        id: String,
        new_filename: String,
    },
    MinimizeToTray,
    OpenAboutDialog,
}

pub fn render_top_bar(
    ui: &mut egui::Ui,
    state: &AppState,
    dialog: &mut NewDownloadDialog,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("⚡ XDM-Rust").strong().color(Color32::from_rgb(0, 168, 255)));
        ui.label(RichText::new("v1.0").strong().color(Color32::from_rgb(0, 230, 118)));
        ui.label(RichText::new("Download Manager").small().color(Color32::GRAY));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("ℹ About").on_hover_text("About XDM-Rust & Open Use Privacy Policy").clicked() {
                actions.push(UiAction::OpenAboutDialog);
            }

            if ui.button("🗕 Tray").on_hover_text("Minimize to System Tray next to clock").clicked() {
                actions.push(UiAction::MinimizeToTray);
            }

            if ui.button(RichText::new("➕ Add URL").strong()).clicked() {
                dialog.is_open = true;
                dialog.url.clear();
                dialog.filename.clear();
                dialog.probed_size = None;
                dialog.error_msg = None;
            }

            if let Some(selected_id) = &state.selected_id {
                if let Some(item) = state.items.iter().find(|i| &i.id == selected_id) {
                    if item.status.is_active() {
                        if ui.button("⏹ Stop").clicked() {
                            actions.push(UiAction::StopDownload(selected_id.clone()));
                        }
                        if ui.button("⏸ Pause").clicked() {
                            actions.push(UiAction::PauseDownload(selected_id.clone()));
                        }
                    } else if item.status != DownloadStatus::Completed {
                        if ui.button("▶ Resume").clicked() {
                            actions.push(UiAction::ResumeDownload(selected_id.clone()));
                        }
                    }

                    if item.status == DownloadStatus::Completed {
                        if ui.button("📂 Open").clicked() {
                            actions.push(UiAction::OpenDownloadedFile(item.save_path.clone()));
                        }
                    }

                    if ui.button("📁 Folder").clicked() {
                        actions.push(UiAction::OpenContainingFolder(item.save_path.clone()));
                    }

                    if ui.button("✏ Rename").clicked() {
                        actions.push(UiAction::OpenRenameDialog {
                            id: selected_id.clone(),
                            current_name: item.filename.clone(),
                        });
                    }

                    if ui.button(RichText::new("🗑 Delete").color(Color32::from_rgb(255, 100, 100))).clicked() {
                        actions.push(UiAction::DeleteDownload {
                            id: selected_id.clone(),
                            delete_file: false,
                        });
                    }
                }
            }
        });
    });
}

pub fn render_sidebar(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<UiAction>) {
    ui.heading(RichText::new("Categories").size(14.0).color(Color32::LIGHT_GRAY));
    ui.add_space(8.0);

    let categories = [
        FileCategory::All,
        FileCategory::Video,
        FileCategory::Audio,
        FileCategory::Compressed,
        FileCategory::Document,
        FileCategory::Program,
        FileCategory::Other,
    ];

    for cat in categories {
        let count = if cat == FileCategory::All {
            state.items.len()
        } else {
            state.items.iter().filter(|i| i.category == cat).count()
        };

        let is_selected = state.active_category == cat;
        let text = format!("{} {} ({})", cat.icon(), cat.display_name(), count);

        let resp = ui.selectable_label(is_selected, text);
        if resp.clicked() {
            state.active_category = cat;
        }
    }

    ui.add_space(20.0);
    ui.separator();
    ui.add_space(10.0);

    // Active status filter pills
    let active_count = state.items.iter().filter(|i| i.status.is_active()).count();
    let complete_count = state
        .items
        .iter()
        .filter(|i| matches!(i.status, DownloadStatus::Completed))
        .count();

    ui.label(RichText::new(format!("Active Transfers: {}", active_count)).color(Color32::GREEN));
    ui.label(RichText::new(format!("Completed: {}", complete_count)).color(Color32::LIGHT_BLUE));

    ui.add_space(24.0);
    ui.separator();
    ui.add_space(8.0);

    ui.label(RichText::new("Version 1.0").small().strong().color(Color32::from_rgb(0, 230, 118)));
    ui.label(RichText::new("Windows 11 x86_64").small().color(Color32::DARK_GRAY));
    ui.add_space(4.0);
    if ui.button(RichText::new("ℹ About & Privacy").small()).clicked() {
        actions.push(UiAction::OpenAboutDialog);
    }
}

pub fn render_download_table(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<UiAction>,
) {
    let filtered_items: Vec<DownloadItem> = state
        .items
        .iter()
        .filter(|item| {
            if state.active_category == FileCategory::All {
                true
            } else {
                item.category == state.active_category
            }
        })
        .cloned()
        .collect();

    if filtered_items.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("No downloads in this view.").size(16.0).color(Color32::GRAY));
            ui.label(RichText::new("Click '+ Add URL' above to start downloading!").small().color(Color32::DARK_GRAY));
        });
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for item in filtered_items {
            let is_selected = state.selected_id.as_deref() == Some(&item.id);

            let bg_color = if is_selected {
                Color32::from_rgb(26, 42, 62)
            } else {
                Color32::from_rgb(18, 20, 26)
            };

            let border_stroke = if is_selected {
                Stroke::new(1.5_f32, Color32::from_rgb(0, 168, 255))
            } else {
                Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 52))
            };

            let item_id = item.id.clone();
            let item_save_path = item.save_path.clone();
            let item_status = item.status.clone();
            let _item_url = item.url.clone();
            let item_filename = item.filename.clone();

            let mut header_clicked = false;
            let mut header_double_clicked = false;

            egui::Frame::none()
                .fill(bg_color)
                .stroke(border_stroke)
                .inner_margin(10.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Category Icon
                        ui.label(RichText::new(item.category.icon()).size(24.0));
                        ui.add_space(4.0);

                        ui.vertical(|ui| {
                            // Top Row: Filename on Left, Status Badge on Right
                            let top_resp = ui.horizontal(|ui| {
                                let filename_text = RichText::new(&item.filename)
                                    .strong()
                                    .size(14.0)
                                    .color(if is_selected {
                                        Color32::from_rgb(230, 245, 255)
                                    } else {
                                        Color32::WHITE
                                    });

                                let lbl = ui.add(egui::Label::new(filename_text).sense(Sense::click()));
                                if lbl.clicked() {
                                    header_clicked = true;
                                }
                                if lbl.double_clicked() {
                                    header_double_clicked = true;
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let status_color = match &item.status {
                                        DownloadStatus::Downloading => Color32::from_rgb(0, 230, 118),
                                        DownloadStatus::Connecting => Color32::from_rgb(255, 214, 0),
                                        DownloadStatus::Paused => Color32::from_rgb(255, 171, 0),
                                        DownloadStatus::Stopped => Color32::from_rgb(255, 138, 101),
                                        DownloadStatus::Completed => Color32::from_rgb(41, 121, 255),
                                        DownloadStatus::Error(_) => Color32::from_rgb(255, 82, 82),
                                        DownloadStatus::Queued => Color32::GRAY,
                                    };
                                    ui.label(RichText::new(item.status.display()).color(status_color).small().strong());
                                });
                            });

                            if top_resp.response.clicked() {
                                header_clicked = true;
                            }

                            ui.add_space(4.0);

                            // Progress Bar
                            let progress = item.progress_fraction();
                            let pb = egui::ProgressBar::new(progress)
                                .show_percentage()
                                .animate(item.status.is_active());
                            let pb_resp = ui.add(pb).interact(Sense::click());
                            if pb_resp.clicked() {
                                header_clicked = true;
                            }

                            ui.add_space(4.0);

                            // Bottom Row: Metrics on Left, Action Buttons on Right!
                            ui.horizontal(|ui| {
                                let stats_resp = ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("Size: {}", item.formatted_size())).small().color(Color32::LIGHT_GRAY));
                                    ui.separator();
                                    ui.label(RichText::new(format!("Speed: {}", item.formatted_speed())).small().color(Color32::from_rgb(0, 220, 130)));
                                    ui.separator();
                                    ui.label(RichText::new(format!("ETA: {}", item.formatted_eta())).small().color(Color32::LIGHT_GRAY));
                                    ui.separator();
                                    ui.label(RichText::new(format!("Parts: {}", item.num_connections)).small().color(Color32::DARK_GRAY));
                                });

                                if stats_resp.response.clicked() {
                                    header_clicked = true;
                                }

                                // Direct In-Row Controls (Right-aligned, unobstructed!)
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // 1. Delete button
                                    if ui.button(RichText::new("🗑 Delete").small().color(Color32::from_rgb(255, 100, 100))).clicked() {
                                        state.selected_id = Some(item_id.clone());
                                        actions.push(UiAction::DeleteDownload {
                                            id: item_id.clone(),
                                            delete_file: false,
                                        });
                                    }

                                    // 2. Folder button
                                    if ui.button(RichText::new("📁 Folder").small()).clicked() {
                                        state.selected_id = Some(item_id.clone());
                                        actions.push(UiAction::OpenContainingFolder(item_save_path.clone()));
                                    }

                                    // 3. Rename button
                                    if ui.button(RichText::new("✏ Rename").small()).clicked() {
                                        state.selected_id = Some(item_id.clone());
                                        actions.push(UiAction::OpenRenameDialog {
                                            id: item_id.clone(),
                                            current_name: item_filename.clone(),
                                        });
                                    }

                                    // 4. Open File button (if completed)
                                    if item_status == DownloadStatus::Completed {
                                        if ui.button(RichText::new("📂 Open").small().color(Color32::from_rgb(0, 200, 255))).clicked() {
                                            state.selected_id = Some(item_id.clone());
                                            actions.push(UiAction::OpenDownloadedFile(item_save_path.clone()));
                                        }
                                    }

                                    // 5. Active: Pause and Stop
                                    if item_status.is_active() {
                                        if ui.button(RichText::new("⏹ Stop").small().color(Color32::from_rgb(255, 120, 120))).clicked() {
                                            state.selected_id = Some(item_id.clone());
                                            actions.push(UiAction::StopDownload(item_id.clone()));
                                        }
                                        if ui.button(RichText::new("⏸ Pause").small().color(Color32::from_rgb(255, 200, 50))).clicked() {
                                            state.selected_id = Some(item_id.clone());
                                            actions.push(UiAction::PauseDownload(item_id.clone()));
                                        }
                                    } else if item_status != DownloadStatus::Completed {
                                        // Paused / Stopped / Error / Queued: Start / Resume
                                        let resume_label = if item_status == DownloadStatus::Paused {
                                            "▶ Resume"
                                        } else {
                                            "▶ Start"
                                        };
                                        if ui.button(RichText::new(resume_label).small().strong().color(Color32::from_rgb(0, 230, 118))).clicked() {
                                            state.selected_id = Some(item_id.clone());
                                            actions.push(UiAction::ResumeDownload(item_id.clone()));
                                        }

                                        if item_status == DownloadStatus::Paused {
                                            if ui.button(RichText::new("⏹ Stop").small().color(Color32::from_rgb(255, 120, 120))).clicked() {
                                                state.selected_id = Some(item_id.clone());
                                                actions.push(UiAction::StopDownload(item_id.clone()));
                                            }
                                        }
                                    }
                                });
                            });
                        });
                    });
                });

            if header_clicked {
                state.selected_id = Some(item.id.clone());
            }

            if header_double_clicked {
                match item.status {
                    DownloadStatus::Completed => {
                        actions.push(UiAction::OpenDownloadedFile(item.save_path.clone()));
                    }
                    DownloadStatus::Downloading | DownloadStatus::Connecting => {
                        actions.push(UiAction::PauseDownload(item.id.clone()));
                    }
                    DownloadStatus::Paused | DownloadStatus::Stopped | DownloadStatus::Queued | DownloadStatus::Error(_) => {
                        actions.push(UiAction::ResumeDownload(item.id.clone()));
                    }
                }
            }

            ui.add_space(6.0);
        }
    });
}

pub fn render_chunk_visualizer(ui: &mut egui::Ui, item: &DownloadItem) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("⚡ Active Connections & Chunk Visualizer:").strong().size(12.0));
            ui.label(RichText::new(format!("{} total connections", item.chunks.len())).small().color(Color32::GRAY));
        });

        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            for chunk in &item.chunks {
                let frac = chunk.fraction();
                let width = 70.0;
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Part #{}", chunk.id + 1)).size(10.0));
                    let color = if chunk.is_finished {
                        Color32::from_rgb(41, 121, 255)
                    } else {
                        Color32::from_rgb(0, 230, 118)
                    };
                    let pb = egui::ProgressBar::new(frac)
                        .desired_width(width)
                        .fill(color);
                    ui.add(pb);
                    ui.label(RichText::new(format_bytes(chunk.downloaded_bytes)).size(9.0).color(Color32::DARK_GRAY));
                });
                ui.add_space(4.0);
            }
        });
    });
}

pub fn render_new_download_modal(
    ctx: &egui::Context,
    dialog: &mut NewDownloadDialog,
    actions: &mut Vec<UiAction>,
) {
    if !dialog.is_open {
        return;
    }

    egui::Window::new("Add New Download")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .default_width(500.0)
        .show(ctx, |ui| {
            ui.label(RichText::new("Enter File URL to Download:").strong());

            ui.horizontal(|ui| {
                let _url_resp = ui.text_edit_singleline(&mut dialog.url);
                if ui.button("📋 Paste").clicked() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            dialog.url = text.trim().to_string();
                        }
                    }
                }
            });

            if !dialog.url.is_empty() && dialog.filename.is_empty() {
                if let Ok(parsed) = url::Url::parse(&dialog.url) {
                    // Check query params for title/filename
                    for (k, v) in parsed.query_pairs() {
                        let key = k.to_ascii_lowercase();
                        if (key == "filename" || key == "title" || key == "name") && !v.trim().is_empty() {
                            dialog.filename = crate::engine::sanitize_filename(&v);
                            break;
                        }
                    }
                    if dialog.filename.is_empty() {
                        if let Some(segments) = parsed.path_segments() {
                            if let Some(last) = segments.last() {
                                if !last.is_empty() {
                                    dialog.filename = crate::engine::sanitize_filename(last);
                                }
                            }
                        }
                    }
                }
                if dialog.filename.is_empty() {
                    dialog.filename = "download.bin".to_string();
                }
            }

            ui.add_space(8.0);
            ui.label(RichText::new("Save As File Name (with extension):").strong());
            ui.text_edit_singleline(&mut dialog.filename);

            ui.add_space(8.0);
            ui.label(RichText::new("Save To Directory:").strong());
            ui.horizontal(|ui| {
                let mut dir_str = dialog.destination_dir.to_string_lossy().to_string();
                ui.text_edit_singleline(&mut dir_str);
                if ui.button("Browse...").clicked() {
                    if let Some(folder) = rfd::FileDialog::new()
                        .set_directory(&dialog.destination_dir)
                        .pick_folder()
                    {
                        dialog.destination_dir = folder;
                    }
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Parallel Connections / Chunks:").strong());
                egui::ComboBox::from_label("")
                    .selected_text(format!("{} streams", dialog.connections))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut dialog.connections, 1, "1 (Single-stream)");
                        ui.selectable_value(&mut dialog.connections, 2, "2 streams");
                        ui.selectable_value(&mut dialog.connections, 4, "4 streams");
                        ui.selectable_value(&mut dialog.connections, 8, "8 streams (Recommended)");
                        ui.selectable_value(&mut dialog.connections, 16, "16 streams (Maximum speed)");
                    });
            });

            if let Some(err) = &dialog.error_msg {
                ui.add_space(4.0);
                ui.label(RichText::new(err).color(Color32::RED).small());
            }

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button(RichText::new("⚡ Start Download").strong().color(Color32::from_rgb(0, 230, 118))).clicked() {
                    if dialog.url.trim().is_empty() {
                        dialog.error_msg = Some("Please enter a valid URL.".to_string());
                    } else {
                        let final_destination = dialog.destination_dir.join(&dialog.filename);
                        actions.push(UiAction::StartNewDownload {
                            url: dialog.url.clone(),
                            filename: dialog.filename.clone(),
                            destination: final_destination,
                            connections: dialog.connections,
                            total_bytes: dialog.probed_size.unwrap_or(0),
                            supports_ranges: dialog.supports_ranges,
                        });
                        dialog.is_open = false;
                    }
                }

                if ui.button("Cancel").clicked() {
                    dialog.is_open = false;
                }
            });
        });
}

pub fn render_about_modal(
    ctx: &egui::Context,
    dialog: &mut AboutDialog,
) {
    if !dialog.is_open {
        return;
    }

    egui::Window::new("ℹ About XDM-Rust Download Manager")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("⚡ XDM-Rust Download Manager").strong().color(Color32::from_rgb(0, 168, 255)));
                ui.label(RichText::new("v1.0").strong().color(Color32::from_rgb(0, 230, 118)));
            });
            ui.label(RichText::new("High-Performance Multi-threaded Download Manager for Windows 11").small().color(Color32::LIGHT_GRAY));
            ui.add_space(6.0);

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Developer:").strong());
                    ui.label(RichText::new("Dr. Vinayak Ashok Bharadi | Google Antigravity & Vinaya Gaurang").color(Color32::WHITE));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Email:").strong());
                    ui.label(RichText::new("vinu.bharadi@gmail.com | vinayagaurang@zohomail.in").color(Color32::from_rgb(0, 190, 255)));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Version:").strong());
                    ui.label(RichText::new("1.0 (Windows x86_64)").color(Color32::from_rgb(0, 230, 118)));
                });
            });

            ui.add_space(8.0);

            // Tab bar for Terms of Use and Privacy Policy
            ui.horizontal(|ui| {
                if ui.selectable_label(dialog.active_tab == AboutTab::TermsOfUse, "📜 Terms of Use & Disclaimer").clicked() {
                    dialog.active_tab = AboutTab::TermsOfUse;
                }
                if ui.selectable_label(dialog.active_tab == AboutTab::PrivacyPolicy, "🛡 Privacy Policy").clicked() {
                    dialog.active_tab = AboutTab::PrivacyPolicy;
                }
            });

            ui.add_space(4.0);

            egui::Frame::none()
                .fill(Color32::from_rgb(14, 16, 20))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 50, 60)))
                .inner_margin(10.0)
                .rounding(6.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        match dialog.active_tab {
                            AboutTab::TermsOfUse => {
                                ui.label(RichText::new("Terms of Use & Disclaimer for XDM-Rust Download Manager\n\
Effective Date: November 23, 2026\n\n\
By downloading, installing, or using XDM-Rust Download Manager, you agree to the following Terms of Use and Disclaimer.\n\n\
1. Free, Non-Commercial Software\n\
XDM-Rust Download Manager is provided completely free of charge on a no-profit, no-loss basis for personal and general use. You may use the software without licensing fees, subscriptions, or hidden charges.\n\n\
2. User Responsibility for Downloaded Content\n\
XDM-Rust Download Manager is a neutral technical utility designed solely to manage and accelerate file downloads from user-provided URLs. By using this application, you agree that:\n\n\
• Legality & Copyright: You are solely responsible for ensuring that you have the legal right to download, store, and use any file or media accessed through the app. You agree not to use the software to infringe upon any copyright, trademark, or intellectual property rights.\n\n\
• File Safety: The developer does not host, index, scan, or verify any files downloaded through the app. You are solely responsible for scanning downloaded files for viruses, malware, or other harmful components before opening or executing them.\n\n\
3. Disclaimer of Warranties (\"As-Is\")\n\
XDM-Rust Download Manager is provided on an \"AS IS\" and \"AS AVAILABLE\" basis, without warranties of any kind—either express or implied—including, but not limited to, implied warranties of merchantability, fitness for a particular purpose, or non-infringement. The developer does not guarantee that the app will be error-free, uninterrupted, or compatible with all websites and servers.\n\n\
4. Limitation of Liability\n\
To the maximum extent permitted by applicable law, the developer shall not be held liable for any direct, indirect, incidental, special, or consequential damages resulting from:\n\
- Your use or inability to use the application\n\
- Any files, data, or content downloaded from third-party sources\n\
- Data loss, system corruption, or security breaches caused by third-party files\n\
- Legal claims arising from your violation of third-party terms of service or copyright laws\n\n\
5. Changes to These Terms\n\
The developer reserves the right to update these Terms of Use at any time. Continued use of the application after any updates constitutes acceptance of the revised terms.\n\n\
6. Contact Information\n\
If you have any questions regarding these Terms of Use, please contact:\n\
Developer: Dr. Vinayak Ashok Bharadi\n\
Email: vinu.bharadi@gmail.com").size(11.0).color(Color32::LIGHT_GRAY));
                            }
                            AboutTab::PrivacyPolicy => {
                                ui.label(RichText::new("Standard Open Use Privacy Policy\n\
Effective Date: November 23, 2026\n\n\
1. 100% Local Execution & Zero Data Collection\n\
XDM-Rust is an open-use, client-side utility running exclusively on your device. It does NOT collect, record, harvest, transmit, or share any personal information, browsing history, downloaded content, or user telemetry.\n\n\
2. Direct Network Communication\n\
All file downloads and network requests are conducted strictly and directly between your computer and the target server URLs specified by you. No intermediate proxy servers or third-party tracking services are ever involved.\n\n\
3. Local Storage Security\n\
Your download queues, task metadata, and temporary part files are stored strictly on your local disk (%APPDATA%\\XDM-Rust). No configuration or usage data is uploaded to remote servers.\n\n\
4. Freedom of Open Use\n\
This software is distributed for open, free, non-commercial and commercial personal use with total transparency and respect for user sovereignty.").size(11.0).color(Color32::LIGHT_GRAY));
                            }
                        }
                    });
                });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Close").strong()).clicked() {
                        dialog.is_open = false;
                    }
                });
            });
        });
}

pub fn render_rename_modal(
    ctx: &egui::Context,
    dialog: &mut RenameDialog,
    actions: &mut Vec<UiAction>,
) {
    if !dialog.is_open {
        return;
    }

    egui::Window::new("✏ Rename Download File")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .default_width(450.0)
        .show(ctx, |ui| {
            ui.label(RichText::new("Specify filename with extension (e.g. MyVideo.mp4, Document.pdf):").strong());
            ui.add_space(6.0);
            ui.text_edit_singleline(&mut dialog.new_name);

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("💾 Save Rename").strong().color(Color32::from_rgb(0, 230, 118))).clicked() {
                    let clean = dialog.new_name.trim().to_string();
                    if !clean.is_empty() {
                        actions.push(UiAction::RenameDownload {
                            id: dialog.item_id.clone(),
                            new_filename: clean,
                        });
                        dialog.is_open = false;
                    }
                }

                if ui.button("Cancel").clicked() {
                    dialog.is_open = false;
                }
            });
        });
}
