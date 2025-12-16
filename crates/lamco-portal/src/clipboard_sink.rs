//! ClipboardSink implementation for Portal clipboard
//!
//! This module provides a [`PortalClipboardSink`] that implements the
//! [`ClipboardSink`] trait from `lamco-clipboard-core`, bridging the
//! abstract clipboard interface to the Portal Clipboard API.
//!
//! # Feature Gate
//!
//! This module requires the `clipboard-sink` feature:
//!
//! ```toml
//! [dependencies]
//! lamco-portal = { version = "0.1", features = ["clipboard-sink"] }
//! ```
//!
//! # Architecture
//!
//! Portal clipboard uses delayed rendering - formats are announced without data.
//! When a local application pastes, Portal sends a `SelectionTransfer` signal
//! with a serial number. We must respond via `SelectionWrite` with that serial.
//!
//! ```text
//! RDP Client copies -> announce_formats() -> Portal SetSelection
//! User pastes in Linux app -> SelectionTransfer signal
//! -> write_clipboard() queues data -> SelectionWrite with serial
//! ```
//!
//! # Example
//!
//! ```ignore
//! use lamco_portal::{PortalManager, ClipboardManager, PortalClipboardSink};
//! use lamco_clipboard_core::ClipboardSink;
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = PortalManager::with_default().await?;
//! let clipboard_mgr = ClipboardManager::new().await?;
//! let session = manager.create_session("s1".to_string(), Some(&clipboard_mgr)).await?;
//!
//! // Create the sink with session reference (session.session is Arc<Mutex<Session>>)
//! let sink = PortalClipboardSink::new(
//!     clipboard_mgr,
//!     session.session,
//! );
//!
//! // Start listeners for transfer requests
//! sink.start_transfer_listener().await?;
//!
//! // Use ClipboardSink trait methods
//! sink.announce_formats(vec!["text/plain".to_string()]).await?;
//! # Ok(())
//! # }
//! ```

use crate::clipboard::{ClipboardManager, SelectionTransferEvent};
use ashpd::desktop::remote_desktop::RemoteDesktop;
use ashpd::desktop::Session;
use lamco_clipboard_core::{
    ClipboardChange, ClipboardChangeReceiver, ClipboardChangeReceiverInner, ClipboardError, ClipboardResult,
    ClipboardSink, FileInfo,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// Pending data for a MIME type, waiting for SelectionTransfer
#[derive(Debug)]
struct PendingData {
    /// The data to write
    data: Vec<u8>,
    /// When this was queued
    queued_at: std::time::Instant,
}

/// Cached file from URI list
#[derive(Debug, Clone)]
struct CachedFile {
    /// Local file path
    path: PathBuf,
}

/// Portal-based implementation of [`ClipboardSink`]
///
/// This wraps a [`ClipboardManager`] and Portal session to provide clipboard
/// operations via the XDG Desktop Portal Clipboard API.
///
/// # Thread Safety
///
/// This type is `Send + Sync` and can be shared across async tasks.
/// Internal state is protected by appropriate synchronization primitives.
///
/// # Delayed Rendering
///
/// Portal clipboard uses delayed rendering - formats are announced without
/// transferring data. Actual data is only transferred when the user pastes.
/// This improves performance for large clipboard contents.
pub struct PortalClipboardSink {
    /// Portal clipboard manager
    clipboard: Arc<ClipboardManager>,

    /// Portal session (needed for all clipboard operations)
    session: Arc<Mutex<Session<'static, RemoteDesktop<'static>>>>,

    /// Channel for clipboard change notifications
    change_tx: mpsc::UnboundedSender<ClipboardChange>,

    /// Receiver end (taken when subscribe_changes is called)
    change_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ClipboardChange>>>>,

    /// Pending data by MIME type, waiting for SelectionTransfer
    pending_data: Arc<RwLock<HashMap<String, PendingData>>>,

    /// Cached file list from last get_file_list call
    cached_files: Arc<RwLock<Vec<CachedFile>>>,

    /// Channel to receive SelectionTransfer events
    transfer_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<SelectionTransferEvent>>>>,

    /// Sender for SelectionTransfer events (kept for listener setup)
    transfer_tx: mpsc::UnboundedSender<SelectionTransferEvent>,
}

impl PortalClipboardSink {
    /// Create a new Portal clipboard sink
    ///
    /// # Arguments
    ///
    /// * `clipboard` - Portal clipboard manager instance
    /// * `session` - Active Portal session (wrapped in Arc<Mutex> for sharing)
    pub fn new(clipboard: ClipboardManager, session: Arc<Mutex<Session<'static, RemoteDesktop<'static>>>>) -> Self {
        let (change_tx, change_rx) = mpsc::unbounded_channel();
        let (transfer_tx, transfer_rx) = mpsc::unbounded_channel();

        Self {
            clipboard: Arc::new(clipboard),
            session,
            change_tx,
            change_rx: Arc::new(Mutex::new(Some(change_rx))),
            pending_data: Arc::new(RwLock::new(HashMap::new())),
            cached_files: Arc::new(RwLock::new(Vec::new())),
            transfer_rx: Arc::new(Mutex::new(Some(transfer_rx))),
            transfer_tx,
        }
    }

    /// Start listening for local clipboard changes
    ///
    /// This should be called once after creating the sink to enable
    /// notifications when the local clipboard changes.
    pub async fn start_change_listener(&self) -> crate::Result<()> {
        let (owner_tx, mut owner_rx) = mpsc::unbounded_channel::<Vec<String>>();

        // Start the Portal's owner changed listener
        self.clipboard.start_owner_changed_listener(owner_tx).await?;

        // Bridge Portal events to ClipboardChange format
        let change_tx = self.change_tx.clone();
        tokio::spawn(async move {
            while let Some(mime_types) = owner_rx.recv().await {
                let change = ClipboardChange::new(mime_types);
                if change_tx.send(change).is_err() {
                    break;
                }
            }
        });

        info!("Portal clipboard change listener started");
        Ok(())
    }

    /// Start listening for SelectionTransfer events (delayed rendering)
    ///
    /// This processes pending data when local apps request clipboard content.
    /// Must be called after creating the sink for write_clipboard to work.
    pub async fn start_transfer_listener(&self) -> crate::Result<()> {
        // Start Portal's SelectionTransfer listener
        self.clipboard
            .start_selection_transfer_listener(self.transfer_tx.clone())
            .await?;

        // Take the receiver
        let transfer_rx = {
            let mut guard = self.transfer_rx.lock().await;
            guard.take()
        };

        let Some(mut transfer_rx) = transfer_rx else {
            return Err(crate::PortalError::clipboard(
                "transfer listener already started".to_string(),
            ));
        };

        // Clone what we need for the spawned task
        let pending_data = Arc::clone(&self.pending_data);
        let clipboard = Arc::clone(&self.clipboard);
        let session = Arc::clone(&self.session);

        tokio::spawn(async move {
            while let Some(event) = transfer_rx.recv().await {
                let mime_type = event.mime_type.clone();
                let serial = event.serial;

                debug!("SelectionTransfer received: mime={}, serial={}", mime_type, serial);

                // Check for pending data for this MIME type
                let data = {
                    let pending = pending_data.read().await;
                    pending.get(&mime_type).map(|p| p.data.clone())
                };

                match data {
                    Some(data) => {
                        // We have data - write it to Portal
                        let session_guard = session.lock().await;
                        match clipboard
                            .write_selection_data(&session_guard, serial, data.clone())
                            .await
                        {
                            Ok(()) => {
                                info!("Provided {} bytes for {} (serial {})", data.len(), mime_type, serial);
                                // Remove from pending after successful write
                                let mut pending = pending_data.write().await;
                                pending.remove(&mime_type);
                            }
                            Err(e) => {
                                error!("Failed to write selection data: {}", e);
                            }
                        }
                    }
                    None => {
                        warn!("No pending data for mime type: {} (serial {})", mime_type, serial);
                        // Notify Portal of failure
                        let session_guard = session.lock().await;
                        let _ = clipboard
                            .portal_clipboard()
                            .selection_write_done(&session_guard, serial, false)
                            .await;
                    }
                }
            }
            info!("SelectionTransfer listener ended");
        });

        info!("SelectionTransfer listener started - delayed rendering enabled");
        Ok(())
    }

    /// Queue data for a MIME type to be written on SelectionTransfer
    ///
    /// This is called internally by write_clipboard. The data is stored and
    /// will be provided to Portal when a SelectionTransfer event arrives.
    async fn queue_pending_data(&self, mime_type: &str, data: Vec<u8>) {
        let mut pending = self.pending_data.write().await;
        pending.insert(
            mime_type.to_string(),
            PendingData {
                data,
                queued_at: std::time::Instant::now(),
            },
        );
        debug!("Queued data for MIME type: {}", mime_type);

        // Clean up stale entries (older than 30 seconds)
        let stale_threshold = std::time::Duration::from_secs(30);
        let now = std::time::Instant::now();
        pending.retain(|mime, pending_data| {
            let age = now.duration_since(pending_data.queued_at);
            if age > stale_threshold {
                debug!("Removing stale pending data for: {}", mime);
                false
            } else {
                true
            }
        });
    }

    /// Parse URI list and cache file information
    async fn parse_and_cache_files(&self, uri_list: &str) -> ClipboardResult<Vec<FileInfo>> {
        let mut files = Vec::new();
        let mut cached = Vec::new();

        for line in uri_list.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse file:// URI
            let path_str = if let Some(path) = line.strip_prefix("file://") {
                // URL decode
                percent_decode(path)
            } else {
                continue;
            };

            let path = PathBuf::from(&path_str);

            // Stat the file
            match tokio::fs::metadata(&path).await {
                Ok(metadata) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone());

                    let info = if metadata.is_dir() {
                        FileInfo::directory(&name)
                    } else {
                        FileInfo::file(&name, metadata.len())
                    };

                    // Add modified time if available
                    let info = if let Ok(modified) = metadata.modified() {
                        if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                            info.with_modified(duration.as_secs())
                        } else {
                            info
                        }
                    } else {
                        info
                    };

                    files.push(info);
                    cached.push(CachedFile { path });
                }
                Err(e) => {
                    warn!("Failed to stat file {}: {}", path_str, e);
                }
            }
        }

        // Update cache
        {
            let mut cache = self.cached_files.write().await;
            *cache = cached;
        }

        Ok(files)
    }

    /// Get downloads directory for writing files
    fn downloads_dir() -> PathBuf {
        // Try XDG_DOWNLOAD_DIR first, fall back to ~/Downloads
        if let Ok(dir) = std::env::var("XDG_DOWNLOAD_DIR") {
            return PathBuf::from(dir);
        }

        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Downloads");
        }

        // Last resort
        PathBuf::from("/tmp")
    }
}

impl ClipboardSink for PortalClipboardSink {
    /// Announce that new clipboard formats are available
    ///
    /// This sets the Portal selection with the given MIME types.
    /// Data is not transferred until requested (delayed rendering).
    async fn announce_formats(&self, mime_types: Vec<String>) -> ClipboardResult<()> {
        if mime_types.is_empty() {
            debug!("No formats to announce");
            return Ok(());
        }

        let session = self.session.lock().await;
        self.clipboard
            .announce_rdp_formats(&session, mime_types.clone())
            .await
            .map_err(|e| ClipboardError::Backend(e.to_string()))?;

        info!("Announced {} formats via Portal", mime_types.len());
        Ok(())
    }

    /// Read clipboard data from the local Wayland clipboard
    ///
    /// Reads the specified MIME type from the Portal's selection.
    async fn read_clipboard(&self, mime_type: &str) -> ClipboardResult<Vec<u8>> {
        let session = self.session.lock().await;
        let data = self
            .clipboard
            .read_local_clipboard(&session, mime_type)
            .await
            .map_err(|e| ClipboardError::Backend(e.to_string()))?;

        debug!("Read {} bytes from Portal clipboard ({})", data.len(), mime_type);
        Ok(data)
    }

    /// Write data to the clipboard for delayed rendering
    ///
    /// This queues the data to be provided when Portal sends a SelectionTransfer
    /// event for this MIME type. The transfer listener must be running.
    ///
    /// # Note
    ///
    /// Call `start_transfer_listener()` before using this method.
    async fn write_clipboard(&self, mime_type: &str, data: Vec<u8>) -> ClipboardResult<()> {
        debug!("Queueing {} bytes for MIME type: {}", data.len(), mime_type);

        self.queue_pending_data(mime_type, data).await;

        Ok(())
    }

    /// Subscribe to clipboard change notifications
    ///
    /// Returns a receiver that yields changes when the local clipboard changes.
    /// Call `start_change_listener()` first to enable notifications.
    async fn subscribe_changes(&self) -> ClipboardResult<ClipboardChangeReceiver> {
        let mut rx_guard = self.change_rx.lock().await;
        match rx_guard.take() {
            Some(rx) => {
                let inner = Box::new(TokioChangeReceiver { rx });
                Ok(ClipboardChangeReceiver::new(inner))
            }
            None => Err(ClipboardError::InvalidState(
                "change subscription already taken".to_string(),
            )),
        }
    }

    /// Get list of files from the clipboard
    ///
    /// Reads the `text/uri-list` MIME type and parses file URIs.
    /// Files are stat'd to get size and metadata.
    async fn get_file_list(&self) -> ClipboardResult<Vec<FileInfo>> {
        // Try to read text/uri-list from clipboard
        let session = self.session.lock().await;
        let uri_data = match self.clipboard.read_local_clipboard(&session, "text/uri-list").await {
            Ok(data) => data,
            Err(_) => {
                // Also try x-special/gnome-copied-files (GNOME file manager format)
                match self
                    .clipboard
                    .read_local_clipboard(&session, "x-special/gnome-copied-files")
                    .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        debug!("No file list in clipboard: {}", e);
                        return Ok(Vec::new());
                    }
                }
            }
        };
        drop(session); // Release lock before parsing

        let uri_list = String::from_utf8(uri_data).map_err(|_| ClipboardError::InvalidUtf8)?;

        self.parse_and_cache_files(&uri_list).await
    }

    /// Read a chunk of a file from the clipboard
    ///
    /// Uses the cached file list from `get_file_list()`.
    /// Files are read directly from the local filesystem.
    async fn read_file_chunk(&self, index: u32, offset: u64, size: u32) -> ClipboardResult<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let cached = self.cached_files.read().await;

        let index_usize = usize::try_from(index)
            .map_err(|_| ClipboardError::InvalidState(format!("file index {} too large", index)))?;

        let file_entry = cached
            .get(index_usize)
            .ok_or_else(|| ClipboardError::InvalidState(format!("file index {} out of range", index)))?;

        let path = &file_entry.path;

        // Open and seek to offset
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to open file: {}", e)))?;

        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to seek: {}", e)))?;

        // Read requested chunk
        let size_usize = usize::try_from(size)
            .map_err(|_| ClipboardError::InvalidState(format!("chunk size {} too large", size)))?;
        let mut buffer = vec![0u8; size_usize];
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to read: {}", e)))?;

        buffer.truncate(bytes_read);

        debug!(
            "Read {} bytes from file {} at offset {}",
            bytes_read,
            path.display(),
            offset
        );

        Ok(buffer)
    }

    /// Write a file received from the remote clipboard
    ///
    /// Files are written to the user's Downloads directory.
    async fn write_file(&self, path: &str, data: Vec<u8>) -> ClipboardResult<()> {
        use tokio::io::AsyncWriteExt;

        // Extract filename from path (remove any directory components for safety)
        let filename = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| ClipboardError::InvalidState("invalid file path".to_string()))?;

        // Sanitize filename (remove potentially dangerous characters)
        let safe_filename: String = filename
            .chars()
            .map(|c| match c {
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect();

        let dest_dir = Self::downloads_dir();

        // Ensure directory exists
        tokio::fs::create_dir_all(&dest_dir)
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to create directory: {}", e)))?;

        let dest_path = dest_dir.join(&safe_filename);

        // Handle filename conflicts by adding suffix
        let final_path = if dest_path.exists() {
            let stem = std::path::Path::new(&safe_filename)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| safe_filename.clone());
            let ext = std::path::Path::new(&safe_filename)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();

            let mut counter = 1;
            loop {
                let candidate = dest_dir.join(format!("{} ({}){}", stem, counter, ext));
                if !candidate.exists() {
                    break candidate;
                }
                counter += 1;
                if counter > 1000 {
                    return Err(ClipboardError::Backend("too many filename conflicts".to_string()));
                }
            }
        } else {
            dest_path
        };

        // Write the file
        let mut file = tokio::fs::File::create(&final_path)
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to create file: {}", e)))?;

        file.write_all(&data)
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to write file: {}", e)))?;

        file.flush()
            .await
            .map_err(|e| ClipboardError::Backend(format!("failed to flush file: {}", e)))?;

        info!("Wrote {} bytes to {}", data.len(), final_path.display());

        Ok(())
    }
}

/// Tokio-based change receiver implementation
struct TokioChangeReceiver {
    rx: mpsc::UnboundedReceiver<ClipboardChange>,
}

impl ClipboardChangeReceiverInner for TokioChangeReceiver {
    fn recv_blocking(&mut self) -> Option<ClipboardChange> {
        self.rx.blocking_recv()
    }

    fn try_recv(&mut self) -> Option<ClipboardChange> {
        self.rx.try_recv().ok()
    }
}

impl std::fmt::Debug for PortalClipboardSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortalClipboardSink")
            .field("clipboard", &"<ClipboardManager>")
            .field("session", &"<Session>")
            .finish()
    }
}

/// Percent-decode a URL path
fn percent_decode(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(char::from(byte));
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_change_creation() {
        let change = ClipboardChange::new(vec!["text/plain".to_string()]);
        assert_eq!(change.mime_types, vec!["text/plain"]);
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("/path/to/file"), "/path/to/file");
        assert_eq!(percent_decode("/path%2Fwith%2Fencoded"), "/path/with/encoded");
    }

    #[test]
    fn test_downloads_dir() {
        let dir = PortalClipboardSink::downloads_dir();
        // Should return a valid path
        assert!(!dir.as_os_str().is_empty());
    }
}
