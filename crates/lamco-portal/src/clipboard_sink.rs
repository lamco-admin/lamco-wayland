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
//! # Example
//!
//! ```no_run
//! use lamco_portal::{PortalManager, ClipboardManager, PortalClipboardSink};
//! use lamco_clipboard_core::ClipboardSink;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = PortalManager::with_default().await?;
//! let clipboard_mgr = ClipboardManager::new().await?;
//! let session = manager.create_session("s1".to_string(), Some(&clipboard_mgr)).await?;
//!
//! // Create the sink with session reference
//! let sink = PortalClipboardSink::new(
//!     clipboard_mgr,
//!     session.session.clone(), // ashpd session
//! );
//!
//! // Use ClipboardSink trait methods
//! sink.announce_formats(vec!["text/plain".to_string()]).await?;
//! # Ok(())
//! # }
//! ```

use crate::clipboard::ClipboardManager;
use ashpd::desktop::remote_desktop::RemoteDesktop;
use ashpd::desktop::Session;
use lamco_clipboard_core::{
    ClipboardChange, ClipboardChangeReceiver, ClipboardChangeReceiverInner, ClipboardResult,
    ClipboardSink, FileInfo,
};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

/// Portal-based implementation of [`ClipboardSink`]
///
/// This wraps a [`ClipboardManager`] and Portal session to provide clipboard
/// operations via the XDG Desktop Portal Clipboard API.
///
/// # Thread Safety
///
/// This type is `Send + Sync` and can be shared across async tasks.
/// The internal session is wrapped in `Arc<Mutex>` to allow concurrent access.
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
}

impl PortalClipboardSink {
    /// Create a new Portal clipboard sink
    ///
    /// # Arguments
    ///
    /// * `clipboard` - Portal clipboard manager instance
    /// * `session` - Active Portal session (wrapped in Arc<Mutex> for sharing)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use lamco_portal::{ClipboardManager, PortalClipboardSink};
    /// # use std::sync::Arc;
    /// # use tokio::sync::Mutex;
    /// # async fn example(session: Arc<Mutex<ashpd::desktop::Session<'static, ashpd::desktop::remote_desktop::RemoteDesktop<'static>>>>) -> crate::Result<()> {
    /// let clipboard_mgr = ClipboardManager::new().await?;
    /// let sink = PortalClipboardSink::new(clipboard_mgr, session);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        clipboard: ClipboardManager,
        session: Arc<Mutex<Session<'static, RemoteDesktop<'static>>>>,
    ) -> Self {
        let (change_tx, change_rx) = mpsc::unbounded_channel();

        Self {
            clipboard: Arc::new(clipboard),
            session,
            change_tx,
            change_rx: Arc::new(Mutex::new(Some(change_rx))),
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
            .map_err(|e| lamco_clipboard_core::ClipboardError::Backend(e.to_string()))?;

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
            .map_err(|e| lamco_clipboard_core::ClipboardError::Backend(e.to_string()))?;

        debug!("Read {} bytes from Portal clipboard ({})", data.len(), mime_type);
        Ok(data)
    }

    /// Write data to the clipboard (in response to SelectionTransfer)
    ///
    /// This is called when a local application requests data from the clipboard
    /// after we've announced formats. The serial must match the transfer request.
    async fn write_clipboard(&self, mime_type: &str, data: Vec<u8>) -> ClipboardResult<()> {
        // For Portal, writes happen in response to SelectionTransfer events
        // which include a serial. This method signature doesn't include serial,
        // so we need to track pending transfers separately.
        //
        // For now, we'll log a warning. A proper implementation needs to:
        // 1. Queue data by MIME type
        // 2. Respond when SelectionTransfer comes in with matching type
        warn!(
            "write_clipboard called for {} ({} bytes) - transfer handling TBD",
            mime_type,
            data.len()
        );

        // TODO: Implement proper transfer response queue
        // This requires tracking pending SelectionTransfer events
        Ok(())
    }

    /// Subscribe to clipboard change notifications
    ///
    /// Returns a receiver that yields changes when the local clipboard changes.
    /// Call [`start_change_listener`] first to enable notifications.
    async fn subscribe_changes(&self) -> ClipboardResult<ClipboardChangeReceiver> {
        let mut rx_guard = self.change_rx.lock().await;
        match rx_guard.take() {
            Some(rx) => {
                let inner = Box::new(TokioChangeReceiver { rx });
                Ok(ClipboardChangeReceiver::new(inner))
            }
            None => Err(lamco_clipboard_core::ClipboardError::InvalidState(
                "Change subscription already taken".to_string(),
            )),
        }
    }

    /// Get list of files in the clipboard
    ///
    /// Portal clipboard doesn't directly support file lists in the same way
    /// as Windows CF_HDROP. Files would need to be read via `text/uri-list` MIME type.
    async fn get_file_list(&self) -> ClipboardResult<Vec<FileInfo>> {
        // Portal uses text/uri-list for file references
        // We'd need to:
        // 1. Check if text/uri-list is available
        // 2. Read and parse the URIs
        // 3. Convert to FileInfo structs

        debug!("get_file_list: Portal file list not implemented yet");
        Ok(Vec::new())
    }

    /// Read a chunk of a file from the clipboard
    ///
    /// This requires the file to be accessible locally (via URI from text/uri-list).
    /// Not directly supported by Portal - files must be read from their local paths.
    async fn read_file_chunk(
        &self,
        index: u32,
        offset: u64,
        size: u32,
    ) -> ClipboardResult<Vec<u8>> {
        debug!(
            "read_file_chunk: index={}, offset={}, size={} - not implemented",
            index, offset, size
        );
        Err(lamco_clipboard_core::ClipboardError::UnsupportedFormat(
            "Portal file chunk read not implemented".to_string(),
        ))
    }

    /// Write a file to the clipboard destination
    ///
    /// For Portal clipboard, this would write to a user-specified location.
    /// Not directly supported - Portal uses drag-and-drop for file transfers.
    async fn write_file(&self, path: &str, data: Vec<u8>) -> ClipboardResult<()> {
        debug!(
            "write_file: path={}, {} bytes - not implemented",
            path,
            data.len()
        );
        Err(lamco_clipboard_core::ClipboardError::UnsupportedFormat(
            "Portal file write not implemented".to_string(),
        ))
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

// Safety: The inner types are Send
unsafe impl Send for TokioChangeReceiver {}

impl std::fmt::Debug for PortalClipboardSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortalClipboardSink")
            .field("clipboard", &"<ClipboardManager>")
            .field("session", &"<Session>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running Wayland session with Portal support
    // They are marked with #[ignore] for CI environments

    #[test]
    fn test_clipboard_change_creation() {
        let change = ClipboardChange::new(vec!["text/plain".to_string()]);
        assert_eq!(change.mime_types, vec!["text/plain"]);
    }
}
