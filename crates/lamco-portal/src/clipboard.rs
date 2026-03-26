//! Portal Clipboard Integration
//!
//! Implements delayed rendering clipboard using Portal Clipboard D-Bus API.
//! This replaces wl-clipboard-rs with proper Portal integration that supports
//! format announcement without data transfer (delayed rendering model).
//!
//! Architecture:
//! - SetSelection() announces available formats to Wayland
//! - SelectionTransfer signal notifies when data is requested
//! - SelectionWrite() provides data via file descriptor
//! - SelectionOwnerChanged signal monitors local clipboard changes
//! - SelectionRead() reads local clipboard data

use std::sync::Arc;

use ashpd::desktop::Session;
use ashpd::desktop::clipboard::{Clipboard, RequestClipboardOptions, SetSelectionOptions};
use ashpd::desktop::remote_desktop::RemoteDesktop;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Selection transfer event from Portal
#[derive(Debug, Clone)]
pub struct SelectionTransferEvent {
    pub mime_type: String,
    pub serial: u32,
}

/// Portal Clipboard Manager
///
/// Integrates RDP clipboard with Wayland via Portal Clipboard API.
/// Supports delayed rendering where formats are announced without data,
/// and data is only transferred when actually requested.
pub struct ClipboardManager {
    /// Portal Clipboard interface (Arc-wrapped for sharing across tasks)
    clipboard: Arc<Clipboard>,
}

impl ClipboardManager {
    /// Create new Portal Clipboard manager
    pub async fn new() -> crate::Result<Self> {
        info!("Initializing Portal Clipboard manager");
        trace!("Creating ashpd Clipboard proxy (uses global D-Bus connection)");

        let clipboard = Clipboard::new().await.map_err(|e| {
            warn!(error = %e, "Failed to create Portal Clipboard proxy");
            crate::PortalError::clipboard(format!("Failed to create Portal Clipboard: {}", e))
        })?;

        let version = clipboard.version();
        info!("Portal Clipboard version: {}", version);

        trace!("ashpd Clipboard proxy created successfully");
        info!("Portal Clipboard manager created (will be enabled when session is ready)");

        let manager = Self {
            clipboard: Arc::new(clipboard),
        };

        Ok(manager)
    }

    /// Get the clipboard portal version
    pub fn version(&self) -> u32 {
        self.clipboard.version()
    }

    /// Start listening for SelectionTransfer events (delayed rendering requests)
    pub async fn start_selection_transfer_listener(
        &self,
        event_tx: mpsc::UnboundedSender<SelectionTransferEvent>,
    ) -> crate::Result<()> {
        let clipboard = Arc::clone(&self.clipboard);

        tokio::spawn(async move {
            use futures_util::stream::StreamExt;

            let stream_result = clipboard.receive_selection_transfer::<RemoteDesktop>().await;

            match stream_result {
                Ok(stream) => {
                    let mut stream = Box::pin(stream);

                    while let Some((_, mime_type, serial)) = stream.next().await {
                        debug!("SelectionTransfer signal: mime={}, serial={}", mime_type, serial);

                        let event = SelectionTransferEvent { mime_type, serial };

                        if event_tx.send(event).is_err() {
                            info!("SelectionTransfer listener stopping (receiver dropped)");
                            break;
                        }
                    }

                    info!("SelectionTransfer listener task ended");
                }
                Err(e) => {
                    info!("Failed to receive SelectionTransfer stream: {:#}", e);
                }
            }
        });

        info!("SelectionTransfer listener started - ready for delayed rendering");
        Ok(())
    }

    /// Start listening for SelectionOwnerChanged events (local clipboard changes)
    pub async fn start_owner_changed_listener(
        &self,
        event_tx: mpsc::UnboundedSender<Vec<String>>,
    ) -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let clipboard = Arc::clone(&self.clipboard);

        tokio::spawn(async move {
            info!("SelectionOwnerChanged listener task starting - attempting to receive stream");
            let stream_result = clipboard.receive_selection_owner_changed::<RemoteDesktop>().await;

            match stream_result {
                Ok(stream) => {
                    info!("SelectionOwnerChanged stream created successfully - waiting for signals");
                    let mut stream = Box::pin(stream);
                    let mut event_count = 0;

                    while let Some((_, change)) = stream.next().await {
                        event_count += 1;
                        info!("SelectionOwnerChanged event #{}: received from Portal", event_count);

                        let is_owner = change.session_is_owner().unwrap_or(false);
                        let mime_types = change.mime_types();

                        info!("   session_is_owner: {}, mime_types: {:?}", is_owner, mime_types);

                        if is_owner {
                            debug!("Ignoring SelectionOwnerChanged - we are the owner");
                            continue;
                        }

                        info!(
                            "Local clipboard changed - new owner has {} formats: {:?}",
                            mime_types.len(),
                            mime_types
                        );

                        if event_tx.send(mime_types.to_vec()).is_err() {
                            info!("SelectionOwnerChanged listener stopping (receiver dropped)");
                            break;
                        }
                    }

                    warn!("SelectionOwnerChanged listener task ended after {} events", event_count);
                }
                Err(e) => {
                    error!("Failed to receive SelectionOwnerChanged stream: {:#}", e);
                    error!("This means Linux->Windows clipboard will NOT work");
                    error!("Portal backend may not support this signal, or permission denied");
                }
            }
        });

        info!("SelectionOwnerChanged listener started - monitoring local clipboard");
        Ok(())
    }

    /// Request clipboard access for session
    ///
    /// Must be called BEFORE the session is started (session state must be INIT).
    pub async fn enable_for_session(&self, session: &Session<RemoteDesktop>) -> crate::Result<()> {
        info!("Requesting clipboard access for session");
        trace!("Calling clipboard.request() - requires session state INIT");

        match self
            .clipboard
            .request(session, RequestClipboardOptions::default())
            .await
        {
            Ok(()) => {
                info!("Portal Clipboard enabled for session");
                Ok(())
            }
            Err(e) => {
                warn!(
                    error = %e,
                    error_debug = ?e,
                    "clipboard.request() failed"
                );
                trace!("Possible causes: state != INIT, already requested, or RD version < 2");
                Err(crate::PortalError::clipboard(format!(
                    "Failed to request clipboard access for session: {}",
                    e
                )))
            }
        }
    }

    /// Announce RDP clipboard formats to Wayland (delayed rendering)
    pub async fn announce_rdp_formats(
        &self,
        session: &Session<RemoteDesktop>,
        mime_types: Vec<String>,
    ) -> crate::Result<()> {
        if mime_types.is_empty() {
            debug!("No formats to announce");
            return Ok(());
        }

        let mime_refs: Vec<&str> = mime_types.iter().map(|s| s.as_str()).collect();

        let options = SetSelectionOptions::default().set_mime_types(&mime_refs);

        self.clipboard
            .set_selection(session, options)
            .await
            .map_err(|e| crate::PortalError::clipboard(format!("Failed to set Portal selection: {}", e)))?;

        info!("Announced {} RDP formats to Portal: {:?}", mime_types.len(), mime_types);
        Ok(())
    }

    /// Get reference to Portal Clipboard for direct API access
    pub fn portal_clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    /// Read from local Wayland clipboard
    #[expect(unsafe_code, reason = "fcntl to clear O_NONBLOCK on portal pipe FD")]
    pub async fn read_local_clipboard(
        &self,
        session: &Session<RemoteDesktop>,
        mime_type: &str,
    ) -> crate::Result<Vec<u8>> {
        use std::io::Read;
        use std::os::fd::AsRawFd;

        debug!("Reading local clipboard: {}", mime_type);

        let fd = self
            .clipboard
            .selection_read(session, mime_type)
            .await
            .map_err(|e| crate::PortalError::clipboard(format!("Failed to get SelectionRead fd: {}", e)))?;

        let std_fd: std::os::fd::OwnedFd = fd.into();
        let mut std_file = std::fs::File::from(std_fd);

        // Portal returns non-blocking pipe FD - set to blocking mode
        let raw_fd = std_file.as_raw_fd();
        // SAFETY: raw_fd is a valid file descriptor from std_file.as_raw_fd()
        let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
        if flags != -1 {
            // SAFETY: raw_fd is valid, clearing O_NONBLOCK for blocking reads
            unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
        }

        let result = tokio::task::spawn_blocking(move || {
            let mut data = Vec::new();
            std_file.read_to_end(&mut data)?;
            Ok::<Vec<u8>, std::io::Error>(data)
        })
        .await
        .map_err(|e| crate::PortalError::clipboard(format!("Join error reading clipboard: {}", e)))?
        .map_err(|e| crate::PortalError::clipboard(format!("I/O error reading clipboard: {}", e)))?;

        info!("Read {} bytes from local clipboard ({})", result.len(), mime_type);
        Ok(result)
    }

    /// Write clipboard data to Portal via file descriptor
    pub async fn write_selection_data(
        &self,
        session: &Session<RemoteDesktop>,
        serial: u32,
        data: Vec<u8>,
    ) -> crate::Result<()> {
        use tokio::io::AsyncWriteExt;

        debug!("Writing {} bytes to Portal clipboard (serial {})", data.len(), serial);

        let fd = self
            .clipboard
            .selection_write(session, serial)
            .await
            .map_err(|e| crate::PortalError::clipboard(format!("Failed to get SelectionWrite fd: {}", e)))?;
        let std_fd: std::os::fd::OwnedFd = fd.into();
        let std_file = std::fs::File::from(std_fd);
        let mut file = tokio::fs::File::from_std(std_file);

        match file.write_all(&data).await {
            Ok(()) => {
                file.flush().await?;
                drop(file);

                self.clipboard
                    .selection_write_done(session, serial, true)
                    .await
                    .map_err(|e| crate::PortalError::clipboard(format!("Failed to notify write completion: {}", e)))?;

                info!("Wrote {} bytes to Portal clipboard (serial {})", data.len(), serial);
                Ok(())
            }
            Err(e) => {
                drop(file);
                let _ = self.clipboard.selection_write_done(session, serial, false).await;
                Err(crate::PortalError::clipboard(format!(
                    "Failed to write clipboard data: {}",
                    e
                )))
            }
        }
    }
}

impl std::fmt::Debug for ClipboardManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortalClipboardManager")
            .field("clipboard", &"<Portal Clipboard Proxy>")
            .finish()
    }
}
