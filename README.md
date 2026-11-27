# ⚡ XDM-Rust Download Manager v1.0 for Windows 11

A high-performance, modern, multi-segmented download manager written in pure Rust for Windows 11 — inspired by Extreme Download Manager (XDM) and Internet Download Manager (IDM).

---

## 👨‍💻 Authors & Maintainers
- **Dr. Vinayak Ashok Bharadi** (`vinu.bharadi@gmail.com`)
- **Vinaya Gaurang** (`vinayagaurang@zohomail.in`)
- **Google Antigravity & Vinaya Gaurang**

---

## 🚀 Key Features

- **Multi-Part / Parallel Acceleration:** Probes remote HTTP servers for `Accept-Ranges: bytes` and splits files across 1 to 32 concurrent HTTP range streams, maxing out your available bandwidth.
- **Dynamic Chunk Visualizer:** Displays live real-time progress bars for every individual chunk connection.
- **Pause, Stop & Resume Continuity:** Chunks stream into partitioned `.dat` part files with byte-offset verification. Downloads can be paused, stopped, and resumed at any time without losing progress or corrupting files.
- **Intelligent Server Filename & Extension Resolution:** Resolves server-specified names via RFC 5987 / RFC 6266 `Content-Disposition`, query parameters (e.g., YouTube `videoplayback?title=...`), and MIME-type fallback mapping to avoid generic stream names.
- **Windows 11 System Tray Docking:** Minimizes to the taskbar notification area next to the clock. Left-click/double-click restores the app, and right-click opens a quick control menu (Pause All, Resume All, Open, Exit).
- **Smart File Categorization:** Auto-sorts files into Categories:
  - 🎬 **Videos** (`.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.flv`)
  - 🎵 **Music & Audio** (`.mp3`, `.wav`, `.flac`, `.aac`, `.ogg`)
  - 🗜️ **Compressed / Archives** (`.zip`, `.rar`, `.7z`, `.tar`, `.gz`, `.iso`)
  - 📄 **Documents** (`.pdf`, `.docx`, `.xlsx`, `.pptx`, `.txt`, `.epub`)
  - ⚙️ **Programs & Installers** (`.exe`, `.msi`, `.apk`, `.dmg`)
  - 📁 **Others**
- **State Persistence:** Preserves download history, unfinished chunks, and settings in `%APPDATA%\XDM-Rust\downloads_state.json`.
- **Native Fluent Dark UI:** Built with pure Rust `eframe`/`egui`, running at native monitor refresh rates with zero web-engine or runtime bloat.
- **100% Local & Zero Telemetry:** No user data collection, no background tracking, and direct client-to-server downloads.

---

## 📦 Project Structure

```text
xdm_rust/
├── src/
│   ├── engine.rs      # Asynchronous multi-part download engine & chunk stitching
│   ├── main.rs        # Application entrypoint, Tokio runtime & eframe loop
│   ├── models.rs      # Data models, chunks, status & file categories
│   ├── state.rs       # AppState management & JSON persistence
│   ├── tray.rs        # Win32 notification area tray integration
│   └── ui.rs          # egui dark-mode dashboard, modals, about & privacy
├── app.ico            # Windows icon resource
├── app_icon.png       # 512x512 master high-resolution logo
├── app_icon_64.rgba   # Raw 64x64 RGBA byte array embedded for eframe window
├── app.rc             # Windows PE VERSIONINFO resource (v1.0.0.0)
├── build.rs           # Cargo build script compiling Windows PE resources
├── Cargo.toml         # Rust crate manifest & dependencies
├── Cargo.lock         # Locked dependency graph
├── installer.iss      # Inno Setup 6 installer script (Windows)
├── xdm-rust.desktop   # Linux desktop entry specification
├── AppRun             # Linux AppImage runtime launch script
├── build-appimage.sh  # Linux AppImage automated packaging script
├── .github/workflows/ # GitHub Actions CI/CD (AppImage automated build)
├── TERMS_OF_USE.txt   # Terms of Use & Disclaimer document
└── README.md          # Project documentation
```

---

## 🛠️ Building & Packaging

### On Windows
```powershell
# 1. Compile optimized release binary
cargo build --release

# 2. Package into Windows Installer (Inno Setup)
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer.iss
```

### On Linux (AppImage Generation)
```bash
# 1. Install prerequisites (Ubuntu/Debian)
sudo apt update && sudo apt install -y libgtk-3-dev libxcb-render0-dev libssl-dev

# 2. Build the universal AppImage
chmod +x build-appimage.sh AppRun
./build-appimage.sh

# Output: XDM-Rust-1.0.0-x86_64.AppImage
```

### Automated GitHub Actions
Pushing to your GitHub repository triggers `.github/workflows/build-appimage.yml`, which automatically builds `XDM-Rust-1.0.0-x86_64.AppImage` on an Ubuntu runner and attaches it as an artifact and GitHub Release.

---

## 📜 License & Terms of Use

This software is distributed free of charge on a non-commercial, no-profit, no-loss basis under the **Terms of Use & Disclaimer** and **Standard Open Use Privacy Policy** contained in `TERMS_OF_USE.txt`.
