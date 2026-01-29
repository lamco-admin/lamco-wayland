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

use ashpd::desktop::clipboard::Clipboard;
use ashpd::desktop::remote_desktop::RemoteDesktop;
use ashpd::desktop::Session;
use std::sync::Arc;
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
    clipboard: Arc<Clipboard<'static>>,
}

impl ClipboardManager {
    /// Create new Portal Clipboard manager
    ///
    /// # Arguments
    ///
    /// * `session` - Reference to RemoteDesktop session to associate clipboard with
    ///
    /// # Returns
    ///
    /// Clipboard manager instance with listeners started
    pub async fn new() -> crate::Result<Self> {
        info!("Initializing Portal Clipboard manager");
        trace!("Creating ashpd Clipboard proxy (uses global D-Bus connection)");

        let clipboard = Clipboard::new().await.map_err(|e| {
            warn!(error = %e, "Failed to create Portal Clipboard proxy");
            crate::PortalError::clipboard(format!("Failed to create Portal Clipboard: {}", e))
        })?;

        trace!("ashpd Clipboard proxy created successfully");
        info!("Portal Clipboard manager created (will be enabled when session is ready)");

        let manager = Self {
            clipboard: Arc::new(clipboard),
        };

        Ok(manager)
    }

    /// Start listening for SelectionTransfer events (delayed rendering requests)
    ///
    /// When a Linux application pastes, Portal sends SelectionTransfer with:
    /// - mime_type: The requested data format
    /// - serial: Unique ID to track this request
    ///
    /// Spawns a background task that listens for SelectionTransfer signals and
    /// sends events to the provided channel.
    pub async fn start_selection_transfer_listener(
        &self,
        event_tx: mpsc::UnboundedSender<SelectionTransferEvent>,
    ) -> crate::Result<()> {
        let clipboard = Arc::clone(&self.clipboard);

        // Task-based stream avoids borrow checker lifetime conflicts with long-lived session
        tokio::spawn(async move {
            use futures_util::stream::StreamExt;

            let stream_result = clipboard.receive_selection_transfer().await;

            match stream_result {
                Ok(stream) => {
                    let mut stream = Box::pin(stream);

                    while let Some((_, mime_type, serial)) = stream.next().await {
                        debug!(
                            "SelectionTransfer signal: mime={}, serial={}",
                            mime_type, serial
                        );

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
    ///
    /// When clipboard ownership changes in the system (user copies in Linux app),
    /// Portal sends SelectionOwnerChanged signal with available MIME types.
    ///
    /// Spawns a background task that listens for these signals and sends events
    /// to the provided channel for announcing to RDP clients.
    pub async fn start_owner_changed_listener(
        &self,
        event_tx: mpsc::UnboundedSender<Vec<String>>,
    ) -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let clipboard = Arc::clone(&self.clipboard);

        tokio::spawn(async move {
            info!("SelectionOwnerChanged listener task starting - attempting to receive stream");
            let stream_result = clipboard.receive_selection_owner_changed().await;

            match stream_result {
                Ok(stream) => {
                    info!(
                        "SelectionOwnerChanged stream created successfully - waiting for signals"
                    );
                    let mut stream = Box::pin(stream);
                    let mut event_count = 0;

                    while let Some((_, change)) = stream.next().await {
                        event_count += 1;
                        info!(
                            "SelectionOwnerChanged event #{}: received from Portal",
                            event_count
                        );

                        // Check if we are the owner (we just set the clipboard)
                        let is_owner = change.session_is_owner().unwrap_or(false);
                        let mime_types = change.mime_types();

                        info!(
                            "   session_is_owner: {}, mime_types: {:?}",
                            is_owner, mime_types
                        );

                        if is_owner {
                            // We own the clipboard (we just announced RDP data) - ignore
                            debug!("Ignoring SelectionOwnerChanged - we are the owner");
                            continue;
                        }

                        // Another application owns the clipboard - announce to RDP clients
                        info!(
                            "Local clipboard changed - new owner has {} formats: {:?}",
                            mime_types.len(),
                            mime_types
                        );

                        if event_tx.send(mime_types).is_err() {
                            info!("SelectionOwnerChanged listener stopping (receiver dropped)");
                            break;
                        }
                    }

                    warn!(
                        "SelectionOwnerChanged listener task ended after {} events",
                        event_count
                    );
                }
                Err(e) => {
                    error!("Failed to receive SelectionOwnerChanged stream: {:#}", e);
                    error!("This means Linux→Windows clipboard will NOT work");
                    error!("Portal backend may not support this signal, or permission denied");
                }
            }
        });

        info!("SelectionOwnerChanged listener started - monitoring local clipboard");
        Ok(())
    }

    /// Request clipboard access for session
    ///
    /// This must be called BEFORE the session is started (session state must be INIT).
    /// Per xdg-desktop-portal source, clipboard.request() requires:
    ///   1. clipboard_requested flag must be FALSE
    ///   2. RemoteDesktop backend version >= 2
    ///   3. Session state must be INIT
    pub async fn enable_for_session(
        &self,
        session: &Session<'_, RemoteDesktop<'_>>,
    ) -> crate::Result<()> {
        info!("Requesting clipboard access for session");
        trace!("Calling clipboard.request() - requires session state INIT");

        match self.clipboard.request(session).await {
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
    ///
    /// When RDP client copies, we announce what formats are available WITHOUT
    /// transferring the actual data. Data is only fetched when user pastes.
    ///
    /// # Arguments
    ///
    /// * `session` - RemoteDesktop session
    /// * `mime_types` - List of MIME types available (e.g., ["text/plain", "image/png"])
    pub async fn announce_rdp_formats(
        &self,
        session: &Session<'_, RemoteDesktop<'_>>,
        mime_types: Vec<String>,
    ) -> crate::Result<()> {
        if mime_types.is_empty() {
            debug!("No formats to announce");
            return Ok(());
        }

        let mime_refs: Vec<&str> = mime_types.iter().map(|s| s.as_str()).collect();

        self.clipboard
            .set_selection(session, &mime_refs)
            .await
            .map_err(|e| {
                crate::PortalError::clipboard(format!("Failed to set Portal selection: {}", e))
            })?;

        info!(
            "Announced {} RDP formats to Portal: {:?}",
            mime_types.len(),
            mime_types
        );
        Ok(())
    }

    /// Get reference to Portal Clipboard for direct API access
    pub fn portal_clipboard(&self) -> &Clipboard<'static> {
        &self.clipboard
    }

    /// Provide clipboard data to Portal via file descriptor (static version for spawned tasks)
    #[allow(dead_code)]
    async fn write_to_portal_fd_static(
        clipboard: &Clipboard<'_>,
        session: &Session<'_, RemoteDesktop<'_>>,
        serial: u32,
        data: &[u8],
    ) -> crate::Result<()> {
        use tokio::io::AsyncWriteExt;

        let fd = clipboard
            .selection_write(session, serial)
            .await
            .map_err(|e| {
                crate::PortalError::clipboard(format!("Failed to get SelectionWrite fd: {}", e))
            })?;

        // zvariant wraps FDs in Fd::Owned enum - extract and convert for tokio async I/O
        let std_fd: std::os::fd::OwnedFd = fd.into();
        let std_file = std::fs::File::from(std_fd);
        let mut file = tokio::fs::File::from_std(std_file);

        file.write_all(data).await?;
        file.flush().await?;
        drop(file);
        clipboard
            .selection_write_done(session, serial, true)
            .await
            .map_err(|e| {
                crate::PortalError::clipboard(format!(
                    "Failed to notify Portal of write completion: {}",
                    e
                ))
            })?;

        info!(
            "Provided {} bytes to Portal (serial {})",
            data.len(),
            serial
        );
        Ok(())
    }

    // Signal streams removed - caller uses portal_clipboard() directly for signal access

    /// Read from local Wayland clipboard
    ///
    /// Used when RDP client requests our clipboard data (Linux → Windows copy).
    ///
    /// # Arguments
    ///
    /// * `session` - RemoteDesktop session
    /// * `mime_type` - MIME type to read (e.g., "text/plain")
    ///
    /// # Returns
    ///
    /// Clipboard data in requested format
    #[allow(unsafe_code)]
    pub async fn read_local_clipboard(
        &self,
        session: &Session<'_, RemoteDesktop<'_>>,
        mime_type: &str,
    ) -> crate::Result<Vec<u8>> {
        use std::io::Read;
        use std::os::fd::AsRawFd;

        debug!("Reading local clipboard: {}", mime_type);

        let fd = self
            .clipboard
            .selection_read(session, mime_type)
            .await
            .map_err(|e| {
                crate::PortalError::clipboard(format!("Failed to get SelectionRead fd: {}", e))
            })?;

        let std_fd: std::os::fd::OwnedFd = fd.into();
        let mut std_file = std::fs::File::from(std_fd);

        // Portal returns non-blocking pipe FD - set to blocking mode to avoid EAGAIN errors.
        let raw_fd = std_file.as_raw_fd();
        // SAFETY: raw_fd is a valid file descriptor from std_file.as_raw_fd()
        // fcntl F_GETFL returns the file status flags or -1 on error
        let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
        if flags != -1 {
            // SAFETY: raw_fd is valid and flags contains the current status
            // We clear O_NONBLOCK to enable blocking reads
            unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
        }

        let result = tokio::task::spawn_blocking(move || {
            let mut data = Vec::new();
            std_file.read_to_end(&mut data)?;
            Ok::<Vec<u8>, std::io::Error>(data)
        })
        .await
        .map_err(|e| crate::PortalError::clipboard(format!("Join error reading clipboard: {}", e)))?
        .map_err(|e| {
            crate::PortalError::clipboard(format!("I/O error reading clipboard: {}", e))
        })?;

        info!(
            "Read {} bytes from local clipboard ({})",
            result.len(),
            mime_type
        );
        Ok(result)
    }

    /// Write clipboard data to Portal via file descriptor
    ///
    /// This is called in response to a SelectionTransfer event.
    /// The data is written to the file descriptor returned by selection_write(),
    /// and then selection_write_done() is called to notify success/failure.
    pub async fn write_selection_data(
        &self,
        session: &Session<'_, RemoteDesktop<'_>>,
        serial: u32,
        data: Vec<u8>,
    ) -> crate::Result<()> {
        use tokio::io::AsyncWriteExt;

        debug!(
            "Writing {} bytes to Portal clipboard (serial {})",
            data.len(),
            serial
        );

        let fd = self
            .clipboard
            .selection_write(session, serial)
            .await
            .map_err(|e| {
                crate::PortalError::clipboard(format!("Failed to get SelectionWrite fd: {}", e))
            })?;
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
                    .map_err(|e| {
                        crate::PortalError::clipboard(format!(
                            "Failed to notify write completion: {}",
                            e
                        ))
                    })?;

                info!(
                    "Wrote {} bytes to Portal clipboard (serial {})",
                    data.len(),
                    serial
                );
                Ok(())
            }
            Err(e) => {
                drop(file);
                let _ = self
                    .clipboard
                    .selection_write_done(session, serial, false)
                    .await;
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
