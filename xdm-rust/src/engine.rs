use crate::models::{ChunkProgress, DownloadEvent, DownloadItem, DownloadStatus, FileCategory};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue, RANGE};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub struct DownloadEngine {
    client: reqwest::Client,
    event_tx: UnboundedSender<DownloadEvent>,
}

impl DownloadEngine {
    pub fn new(event_tx: UnboundedSender<DownloadEvent>) -> Self {
        let mut default_headers = HeaderMap::new();
        // Prevent servers from sending gzip/brotli/zstd compressed partial streams
        default_headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(30))
            .default_headers(default_headers)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client, event_tx }
    }

    pub async fn probe_url(&self, url: &str) -> Result<UrlMetadata, String> {
        let mut filename = String::new();
        let mut total_bytes = 0u64;
        let mut supports_ranges = false;

        // Step 1: Try HTTP HEAD probe
        if let Ok(resp) = self.client.head(url).send().await {
            filename = extract_server_filename(&resp, url);
            total_bytes = resp
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);

            supports_ranges = resp
                .headers()
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_lowercase().contains("bytes"))
                .unwrap_or(false);
        }

        // Step 2: If total_bytes is still 0, probe with Range: bytes=0-0
        // Many modern CDNs or reverse proxies omit Content-Length on HEAD
        if total_bytes == 0 {
            if let Ok(resp) = self
                .client
                .get(url)
                .header(RANGE, "bytes=0-0")
                .send()
                .await
            {
                if filename.is_empty() || filename == "download.bin" {
                    let server_fn = extract_server_filename(&resp, url);
                    if !server_fn.is_empty() && server_fn != "download.bin" {
                        filename = server_fn;
                    }
                }

                if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                    supports_ranges = true;
                    // Format: bytes 0-0/123456789
                    if let Some(cr) = resp.headers().get("content-range").and_then(|v| v.to_str().ok()) {
                        if let Some(pos) = cr.rfind('/') {
                            let total_part = cr[pos + 1..].trim();
                            if let Ok(parsed_total) = total_part.parse::<u64>() {
                                total_bytes = parsed_total;
                            }
                        }
                    }
                } else if resp.status().is_success() {
                    if let Some(cl) = resp
                        .headers()
                        .get(CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                    {
                        total_bytes = cl;
                    }
                }
            }
        }

        if filename.is_empty() {
            filename = extract_filename_from_url(url);
        }

        Ok(UrlMetadata {
            filename,
            total_bytes,
            supports_ranges,
        })
    }

    pub async fn execute_download(
        &self,
        mut item: DownloadItem,
        cancel_flag: Arc<AtomicBool>,
    ) {
        let id = item.id.clone();
        let _ = self.event_tx.send(DownloadEvent::StatusChanged(
            id.clone(),
            DownloadStatus::Connecting,
        ));

        // Create destination directory if needed
        if let Some(parent) = item.save_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // If total_bytes or filename was generic/unknown, probe now
        let needs_better_name = item.filename == "download.bin"
            || item.filename.is_empty()
            || !item.filename.contains('.')
            || item.filename.starts_with("videoplayback")
            || item.filename.starts_with("stream");

        if item.total_bytes == 0 || needs_better_name {
            if let Ok(meta) = self.probe_url(&item.url).await {
                if item.total_bytes == 0 && meta.total_bytes > 0 {
                    item.total_bytes = meta.total_bytes;
                    item.supports_ranges = meta.supports_ranges;
                }
                if needs_better_name && !meta.filename.is_empty() && meta.filename != "download.bin" {
                    let parent = item.save_path.parent().unwrap_or_else(|| Path::new("."));
                    item.filename = meta.filename.clone();
                    item.save_path = parent.join(&meta.filename);
                    item.category = FileCategory::from_filename(&item.filename);
                }
            }
        }

        // Broadcast probed metadata back to AppState so UI progress bar, filename and sizes are accurate
        let _ = self.event_tx.send(DownloadEvent::MetadataProbed {
            id: id.clone(),
            total_bytes: item.total_bytes,
            supports_ranges: item.supports_ranges,
            filename: item.filename.clone(),
        });

        let parts_dir = item
            .save_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(".xdm_parts_{}", id));

        let _ = fs::create_dir_all(&parts_dir);

        let num_connections = if item.supports_ranges && item.total_bytes > 0 {
            item.num_connections.clamp(1, 16)
        } else {
            1
        };

        let mut chunks = item.chunks;
        if chunks.is_empty() && item.total_bytes > 0 && num_connections > 1 {
            let chunk_size = item.total_bytes / (num_connections as u64);
            for i in 0..num_connections {
                let start_byte = (i as u64) * chunk_size;
                let end_byte = if i == num_connections - 1 {
                    item.total_bytes - 1
                } else {
                    ((i + 1) as u64) * chunk_size - 1
                };

                chunks.push(ChunkProgress {
                    id: i,
                    start_byte,
                    end_byte,
                    downloaded_bytes: 0,
                    is_finished: false,
                });
            }
        } else if chunks.is_empty() {
            chunks.push(ChunkProgress {
                id: 0,
                start_byte: 0,
                end_byte: if item.total_bytes > 0 { item.total_bytes - 1 } else { 0 },
                downloaded_bytes: 0,
                is_finished: false,
            });
        }

        let _ = self.event_tx.send(DownloadEvent::StatusChanged(
            id.clone(),
            DownloadStatus::Downloading,
        ));

        let mut handles = Vec::new();
        let chunk_count = chunks.len();

        let shared_downloaded = Arc::new(std::sync::atomic::AtomicU64::new(item.downloaded_bytes));
        let chunks_state = Arc::new(tokio::sync::RwLock::new(chunks.clone()));

        for chunk_idx in 0..chunk_count {
            let client = self.client.clone();
            let url = item.url.clone();
            let parts_dir = parts_dir.clone();
            let cancel = cancel_flag.clone();
            let shared_bytes = shared_downloaded.clone();
            let chunks_ref = chunks_state.clone();
            let event_tx_clone = self.event_tx.clone();
            let item_id_clone = id.clone();

            let handle = tokio::spawn(async move {
                let (start_byte, mut end_byte) = {
                    let guard = chunks_ref.read().await;
                    let c = &guard[chunk_idx];
                    (c.start_byte, c.end_byte)
                };

                let part_file_path = parts_dir.join(format!("part_{}.dat", chunk_idx));

                // Open part file with read/write access
                let mut file = match OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .open(&part_file_path)
                {
                    Ok(f) => f,
                    Err(e) => return Err(format!("Failed to open chunk file {}: {}", chunk_idx, e)),
                };

                // Check existing part file size for resuming
                let mut downloaded = file.metadata().map(|m| m.len()).unwrap_or(0);
                {
                    let mut guard = chunks_ref.write().await;
                    guard[chunk_idx].downloaded_bytes = downloaded;
                }

                // Resilient chunk fetch loop with automatic retries and range resumption
                let mut attempts = 0usize;
                const MAX_ATTEMPTS: usize = 5;

                loop {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = file.flush();
                        return Ok::<(), String>(());
                    }

                    let expected_bytes = if end_byte >= start_byte && end_byte > 0 {
                        end_byte - start_byte + 1
                    } else {
                        0
                    };

                    if expected_bytes > 0 && downloaded >= expected_bytes {
                        let mut guard = chunks_ref.write().await;
                        guard[chunk_idx].is_finished = true;
                        let _ = file.flush();
                        return Ok(());
                    }

                    let current_start = start_byte + downloaded;
                    let mut req = client.get(&url);
                    if end_byte > 0 && end_byte >= current_start {
                        req = req.header(RANGE, format!("bytes={}-{}", current_start, end_byte));
                    }

                    let resp = match req.send().await {
                        Ok(r) => r,
                        Err(e) => {
                            attempts += 1;
                            if attempts >= MAX_ATTEMPTS {
                                return Err(format!(
                                    "Chunk {} connect failed after {} attempts: {}",
                                    chunk_idx, attempts, e
                                ));
                            }
                            tokio::time::sleep(Duration::from_millis(500 * attempts as u64)).await;
                            continue;
                        }
                    };

                    if !resp.status().is_success()
                        && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT
                    {
                        attempts += 1;
                        if attempts >= MAX_ATTEMPTS {
                            return Err(format!(
                                "Chunk {} server error: HTTP {}",
                                chunk_idx,
                                resp.status()
                            ));
                        }
                        tokio::time::sleep(Duration::from_millis(500 * attempts as u64)).await;
                        continue;
                    }

                    // Extract server-specified filename if we discover it now
                    let server_filename = extract_server_filename(&resp, &url);
                    if !server_filename.is_empty() && server_filename != "download.bin" {
                        let _ = event_tx_clone.send(DownloadEvent::MetadataProbed {
                            id: item_id_clone.clone(),
                            total_bytes: 0, // don't overwrite total_bytes unless positive
                            supports_ranges: true,
                            filename: server_filename,
                        });
                    }

                    // If total_bytes was unknown, check Content-Length from server
                    if end_byte == 0 {
                        if let Some(cl) = resp.headers().get(CONTENT_LENGTH)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                        {
                            if cl > 0 {
                                end_byte = cl.saturating_sub(1);
                                let mut guard = chunks_ref.write().await;
                                guard[chunk_idx].end_byte = end_byte;
                                let _ = event_tx_clone.send(DownloadEvent::MetadataProbed {
                                    id: item_id_clone.clone(),
                                    total_bytes: cl,
                                    supports_ranges: false,
                                    filename: String::new(),
                                });
                            }
                        }
                    }

                    // Seek to the end of file to append new bytes
                    if let Err(e) = file.seek(SeekFrom::End(0)) {
                        return Err(format!("Seek error on chunk {}: {}", chunk_idx, e));
                    }

                    let mut stream = resp.bytes_stream();
                    let mut stream_interrupted = false;

                    while let Some(item) = stream.next().await {
                        if cancel.load(Ordering::Relaxed) {
                            let _ = file.flush();
                            return Ok(());
                        }

                        match item {
                            Ok(bytes) => {
                                let len = bytes.len() as u64;
                                if let Err(e) = file.write_all(&bytes) {
                                    return Err(format!(
                                        "Write error on chunk {}: {}",
                                        chunk_idx, e
                                    ));
                                }
                                shared_bytes.fetch_add(len, Ordering::Relaxed);
                                downloaded += len;

                                let mut guard = chunks_ref.write().await;
                                guard[chunk_idx].downloaded_bytes = downloaded;
                                if guard[chunk_idx].total_bytes() > 0
                                    && downloaded >= guard[chunk_idx].total_bytes()
                                {
                                    guard[chunk_idx].is_finished = true;
                                }
                            }
                            Err(_e) => {
                                // Connection dropped or decode timeout on chunk body
                                let _ = file.flush();
                                stream_interrupted = true;
                                break;
                            }
                        }
                    }

                    let _ = file.flush();

                    // Verify on disk bytes
                    let disk_len = file.metadata().map(|m| m.len()).unwrap_or(downloaded);
                    let target_chunk_bytes = if end_byte >= start_byte && end_byte > 0 {
                        end_byte - start_byte + 1
                    } else {
                        0
                    };

                    // Check for premature EOF
                    if target_chunk_bytes > 0 && disk_len < target_chunk_bytes {
                        stream_interrupted = true;
                    }

                    if stream_interrupted {
                        attempts += 1;
                        if attempts >= MAX_ATTEMPTS {
                            return Err(format!(
                                "Chunk {} incomplete ({} / {} bytes downloaded) after {} attempts",
                                chunk_idx, disk_len, target_chunk_bytes, attempts
                            ));
                        }
                        tokio::time::sleep(Duration::from_millis(500 * attempts as u64)).await;
                        downloaded = disk_len;
                        continue;
                    }

                    // Successfully completed chunk
                    let mut guard = chunks_ref.write().await;
                    guard[chunk_idx].downloaded_bytes = disk_len;
                    guard[chunk_idx].is_finished = true;
                    return Ok(());
                }
            });

            handles.push(handle);
        }

        // Monitoring task for Speed & ETA
        let monitor_id = id.clone();
        let monitor_event_tx = self.event_tx.clone();
        let monitor_cancel = cancel_flag.clone();
        let monitor_shared_bytes = shared_downloaded.clone();
        let monitor_chunks = chunks_state.clone();
        let total_bytes = item.total_bytes;

        let monitor_handle = tokio::spawn(async move {
            let mut last_bytes = monitor_shared_bytes.load(Ordering::Relaxed);
            let mut last_time = Instant::now();

            while !monitor_cancel.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(400)).await;

                let current_bytes = monitor_shared_bytes.load(Ordering::Relaxed);
                let now = Instant::now();
                let elapsed_secs = now.duration_since(last_time).as_secs_f64();

                if elapsed_secs > 0.0 {
                    let speed = (current_bytes.saturating_sub(last_bytes) as f64) / elapsed_secs;
                    let eta = if speed > 0.0 && total_bytes > current_bytes {
                        Some(((total_bytes - current_bytes) as f64 / speed) as u64)
                    } else {
                        None
                    };

                    let snapshot_chunks = {
                        let guard = monitor_chunks.read().await;
                        guard.clone()
                    };

                    let _ = monitor_event_tx.send(DownloadEvent::ProgressUpdated {
                        id: monitor_id.clone(),
                        downloaded_bytes: current_bytes,
                        total_bytes,
                        speed_bytes_per_sec: speed,
                        eta_seconds: eta,
                        chunks: snapshot_chunks,
                    });

                    last_bytes = current_bytes;
                    last_time = now;
                }
            }
        });

        // Await all chunk download tasks
        let mut has_errors = false;
        let mut err_msg = String::new();

        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    has_errors = true;
                    err_msg = e;
                }
                Err(e) => {
                    has_errors = true;
                    err_msg = format!("Task join error: {}", e);
                }
            }
        }

        monitor_handle.abort();

        if cancel_flag.load(Ordering::Relaxed) {
            let _ = self.event_tx.send(DownloadEvent::Cancelled(id.clone()));
            return;
        }

        if has_errors {
            let _ = self.event_tx.send(DownloadEvent::StatusChanged(
                id.clone(),
                DownloadStatus::Error(err_msg),
            ));
            return;
        }

        // Verify and stitch the parts into the final target file
        let final_path = item.save_path.clone();
        let chunks_snapshot = {
            let guard = chunks_state.read().await;
            guard.clone()
        };

        let stitch_res = tokio::task::spawn_blocking(move || {
            // 1. Rigorous pre-stitch verification of all chunk parts on disk
            let mut total_part_bytes = 0u64;
            for (idx, chunk) in chunks_snapshot.iter().enumerate() {
                let part_path = parts_dir.join(format!("part_{}.dat", idx));
                let meta = fs::metadata(&part_path)
                    .map_err(|e| format!("Missing part file for chunk {}: {}", idx, e))?;
                let expected = chunk.total_bytes();
                if expected > 0 && meta.len() < expected {
                    return Err(format!(
                        "Chunk {} is incomplete: {} bytes downloaded out of {} bytes",
                        idx, meta.len(), expected
                    ));
                }
                total_part_bytes += meta.len();
            }

            // 2. Assemble to a temporary file in destination directory
            let parent_dir = final_path.parent().unwrap_or_else(|| Path::new("."));
            let temp_final = parent_dir.join(format!(".xdm_assembling_{}.tmp", uuid::Uuid::new_v4().simple()));

            {
                let mut out = File::create(&temp_final)
                    .map_err(|e| format!("Failed to create destination file: {}", e))?;

                let mut buffer = vec![0u8; 256 * 1024]; // 256 KB high-throughput buffer

                for idx in 0..chunks_snapshot.len() {
                    let part_path = parts_dir.join(format!("part_{}.dat", idx));
                    let mut part_file = File::open(&part_path)
                        .map_err(|e| format!("Failed to read chunk part {}: {}", idx, e))?;

                    loop {
                        let read = part_file
                            .read(&mut buffer)
                            .map_err(|e| format!("Failed reading chunk {}: {}", idx, e))?;
                        if read == 0 {
                            break;
                        }
                        out.write_all(&buffer[..read])
                            .map_err(|e| format!("Failed writing target file: {}", e))?;
                    }
                }

                out.flush()
                    .map_err(|e| format!("Failed to flush final file: {}", e))?;
            } // Handles dropped here

            // 3. Verify assembled file size
            let assembled_len = fs::metadata(&temp_final).map(|m| m.len()).unwrap_or(0);
            if total_part_bytes > 0 && assembled_len != total_part_bytes {
                let _ = fs::remove_file(&temp_final);
                return Err(format!(
                    "Assembly verification error: assembled {} bytes, expected {} bytes",
                    assembled_len, total_part_bytes
                ));
            }

            // 4. Atomically move/rename temporary assembled file to target final_path
            if final_path.exists() {
                let _ = fs::remove_file(&final_path);
            }
            fs::rename(&temp_final, &final_path)
                .map_err(|e| format!("Failed to finalize destination file: {}", e))?;

            // 5. Cleanup part files and temporary parts directory
            for idx in 0..chunks_snapshot.len() {
                let part_path = parts_dir.join(format!("part_{}.dat", idx));
                let _ = fs::remove_file(&part_path);
            }
            let _ = fs::remove_dir_all(&parts_dir);

            Ok::<(), String>(())
        })
        .await;

        match stitch_res {
            Ok(Ok(())) => {
                let _ = self.event_tx.send(DownloadEvent::Completed(id.clone()));
            }
            Ok(Err(e)) => {
                let _ = self.event_tx.send(DownloadEvent::StatusChanged(
                    id.clone(),
                    DownloadStatus::Error(e),
                ));
            }
            Err(e) => {
                let _ = self.event_tx.send(DownloadEvent::StatusChanged(
                    id.clone(),
                    DownloadStatus::Error(format!("Assembly task panic: {}", e)),
                ));
            }
        }
    }
}

pub struct UrlMetadata {
    pub filename: String,
    pub total_bytes: u64,
    pub supports_ranges: bool,
}

pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex_str = [c1, c2];
                if let Ok(hex) = std::str::from_utf8(&hex_str) {
                    if let Ok(val) = u8::from_str_radix(hex, 16) {
                        bytes.push(val);
                        continue;
                    }
                }
                bytes.push(b'%');
                bytes.push(c1);
                bytes.push(c2);
            } else {
                bytes.push(b'%');
            }
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

pub fn sanitize_filename(name: &str) -> String {
    let decoded = percent_decode(name);
    let mut clean: String = decoded
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '\0'..='\x1f' => '_',
            c => c,
        })
        .collect();

    // Strip leading/trailing spaces and dots
    clean = clean.trim().trim_matches('.').to_string();

    // Check Windows reserved device names
    let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6"
        | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6"
        | "LPT7" | "LPT8" | "LPT9" => {
            clean = format!("_{}", clean);
        }
        _ => {}
    }

    if clean.is_empty() {
        "download.bin".to_string()
    } else {
        clean
    }
}

pub fn extension_from_mime(mime: &str) -> Option<&'static str> {
    let lower = mime.to_ascii_lowercase();
    let content_type = lower.split(';').next().unwrap_or("").trim();
    match content_type {
        "video/mp4" | "video/mp4v-es" => Some(".mp4"),
        "video/webm" => Some(".webm"),
        "video/x-matroska" | "video/mkv" => Some(".mkv"),
        "video/quicktime" => Some(".mov"),
        "video/x-msvideo" => Some(".avi"),
        "video/x-flv" => Some(".flv"),
        "video/mpeg" => Some(".mpeg"),
        "audio/mpeg" | "audio/mp3" => Some(".mp3"),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some(".m4a"),
        "audio/ogg" | "audio/vorbis" => Some(".ogg"),
        "audio/flac" | "audio/x-flac" => Some(".flac"),
        "audio/wav" | "audio/x-wav" => Some(".wav"),
        "audio/aac" => Some(".aac"),
        "application/pdf" => Some(".pdf"),
        "application/zip" | "application/x-zip-compressed" => Some(".zip"),
        "application/x-rar-compressed" | "application/vnd.rar" | "application/x-rar" => Some(".rar"),
        "application/x-7z-compressed" => Some(".7z"),
        "application/x-tar" => Some(".tar"),
        "application/gzip" | "application/x-gzip" => Some(".gz"),
        "application/vnd.android.package-archive" => Some(".apk"),
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => Some(".exe"),
        "application/x-msi" => Some(".msi"),
        "application/json" => Some(".json"),
        "application/xml" | "text/xml" => Some(".xml"),
        "text/plain" => Some(".txt"),
        "text/html" => Some(".html"),
        "text/csv" => Some(".csv"),
        "image/jpeg" => Some(".jpg"),
        "image/png" => Some(".png"),
        "image/webp" => Some(".webp"),
        "image/gif" => Some(".gif"),
        "image/svg+xml" => Some(".svg"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some(".docx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(".xlsx"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some(".pptx"),
        _ => None,
    }
}

pub fn extract_server_filename(resp: &reqwest::Response, original_url: &str) -> String {
    let mut candidate = String::new();

    // 1. Check Content-Disposition header
    if let Some(val) = resp.headers().get(CONTENT_DISPOSITION) {
        if let Ok(s) = val.to_str() {
            // First check filename*= (RFC 5987 / RFC 6266, e.g. UTF-8''encoded_name.ext)
            if let Some(idx) = s.find("filename*=") {
                let raw = &s[idx + 10..];
                let clean = raw.split(';').next().unwrap_or("").trim();
                let without_charset = if let Some(q_idx) = clean.rfind("''") {
                    &clean[q_idx + 2..]
                } else {
                    clean
                };
                let decoded = percent_decode(without_charset.trim_matches(|c| c == '"' || c == '\''));
                if !decoded.trim().is_empty() {
                    candidate = decoded;
                }
            }

            // Fallback to standard filename="name.ext"
            if candidate.is_empty() {
                if let Some(idx) = s.find("filename=") {
                    let raw = &s[idx + 9..];
                    let clean = raw.split(';').next().unwrap_or("").trim();
                    let trimmed = clean.trim_matches(|c| c == '"' || c == '\'');
                    let decoded = percent_decode(trimmed);
                    if !decoded.trim().is_empty() {
                        candidate = decoded;
                    }
                }
            }
        }
    }

    // 2. Check query parameters on the final response URL and original URL
    if candidate.is_empty() {
        for url_to_check in [resp.url().as_str(), original_url] {
            if let Ok(parsed) = url::Url::parse(url_to_check) {
                for (key, val) in parsed.query_pairs() {
                    let k = key.to_ascii_lowercase();
                    if k == "filename" || k == "file" || k == "title" || k == "name" {
                        if !val.trim().is_empty() {
                            candidate = percent_decode(&val);
                            break;
                        }
                    } else if k == "response-content-disposition" {
                        if let Some(idx) = val.find("filename=") {
                            let part = &val[idx + 9..];
                            let raw_name = part.split(';').next().unwrap_or("").trim_matches(|c| c == '"' || c == '\'');
                            if !raw_name.is_empty() {
                                candidate = percent_decode(raw_name);
                                break;
                            }
                        }
                    }
                }
            }
            if !candidate.is_empty() {
                break;
            }
        }
    }

    // 3. Check the path segments of the final redirected response URL
    if candidate.is_empty() {
        if let Some(segments) = resp.url().path_segments() {
            if let Some(last) = segments.last() {
                let trimmed = last.trim();
                if !trimmed.is_empty() && trimmed != "/" {
                    candidate = percent_decode(trimmed);
                }
            }
        }
    }

    // 4. Check the path segments of the original URL
    if candidate.is_empty() {
        if let Ok(parsed) = url::Url::parse(original_url) {
            if let Some(segments) = parsed.path_segments() {
                if let Some(last) = segments.last() {
                    let trimmed = last.trim();
                    if !trimmed.is_empty() && trimmed != "/" {
                        candidate = percent_decode(trimmed);
                    }
                }
            }
        }
    }

    // Sanitize the candidate
    let mut filename = sanitize_filename(&candidate);

    // 5. If filename is generic or missing an extension, check Content-Type header
    let content_type = resp.headers().get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let has_ext = filename.rfind('.').map(|i| i > 0 && i < filename.len() - 1).unwrap_or(false);
    let is_generic = filename == "download.bin"
        || filename == "videoplayback"
        || filename == "stream"
        || filename == "blob";

    if let Some(ext) = extension_from_mime(content_type) {
        if !has_ext || is_generic {
            if is_generic {
                filename = format!("media_stream{}", ext);
            } else if !filename.ends_with(ext) {
                filename.push_str(ext);
            }
        }
    }

    if filename.is_empty() || filename == "." {
        filename = "download.bin".to_string();
    }

    filename
}

fn extract_filename_from_url(url_str: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url_str) {
        // First check query string for filename
        for (k, v) in parsed.query_pairs() {
            let key = k.to_ascii_lowercase();
            if (key == "filename" || key == "title" || key == "name") && !v.trim().is_empty() {
                return sanitize_filename(&v);
            }
        }
        if let Some(segments) = parsed.path_segments() {
            if let Some(last) = segments.last() {
                if !last.trim().is_empty() {
                    return sanitize_filename(last);
                }
            }
        }
    }
    "download.bin".to_string()
}
