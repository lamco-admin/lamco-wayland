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
    ///
    /// The signal is read raw rather than through ashpd's typed stream. xdg-desktop-portal-gnome
    /// 47 to 49 forwards mutter's `mime-types` as a struct wrapping the array, `(as)`, where the
    /// portal specification has `as` (xdg-desktop-portal-gnome issue 136, fixed in 50). ashpd's
    /// stream requires `as` and drops a body that doesn't match without a word, so on RHEL 10 and
    /// other GNOMEs with that backend the desktop's copies never reached the client.
    /// The listener takes either shape and says when it can't read one.
    pub async fn start_owner_changed_listener(
        &self,
        event_tx: mpsc::UnboundedSender<Vec<String>>,
    ) -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let clipboard = Arc::clone(&self.clipboard);

        tokio::spawn(async move {
            info!("SelectionOwnerChanged listener task starting - attempting to receive stream");
            // ashpd's Clipboard derefs to the zbus proxy on ashpd's own connection, the one the
            // session was created on and the portal sends this signal to.
            let stream_result = clipboard.receive_signal("SelectionOwnerChanged").await;

            match stream_result {
                Ok(mut stream) => {
                    info!("SelectionOwnerChanged stream created successfully - waiting for signals");
                    let mut event_count = 0;

                    while let Some(message) = stream.next().await {
                        let body = message.body();
                        let Some((is_owner, mime_types)) = owner_change_from_body(&body) else {
                            warn!(
                                "SelectionOwnerChanged with a body this listener can't read (signature {:?}); ignored",
                                body.signature().to_string()
                            );
                            continue;
                        };
                        event_count += 1;
                        info!("SelectionOwnerChanged event #{}: received from Portal", event_count);
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

                        if event_tx.send(mime_types).is_err() {
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

/// Reads a SelectionOwnerChanged body, `(o, a{sv})`: whether this session owns the selection
/// (`session_is_owner`, false when absent) and the MIME types (`mime_types`). The types come as
/// `as` from a backend that follows the specification and as `(as)` from xdg-desktop-portal-gnome
/// 47 to 49; both are taken. None when the body isn't `(o, a{sv})` at all.
fn owner_change_from_body(body: &zbus::message::Body) -> Option<(bool, Vec<String>)> {
    use std::collections::HashMap;

    use zbus::{OwnedObjectPath, OwnedVariant};

    let (_session, options): (OwnedObjectPath, HashMap<String, OwnedVariant>) = body.deserialize().ok()?;
    let is_owner = options
        .get("session_is_owner")
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false);
    let mime_types = options
        .get("mime_types")
        .map(|v| mime_types_from_value(v))
        .unwrap_or_default();
    Some((is_owner, mime_types))
}

/// The strings in `as`, or in a struct whose first field is `as`.
fn mime_types_from_value(value: &zbus::Variant<'_>) -> Vec<String> {
    use zbus::Variant as Value;
    match value {
        Value::Array(array) => array
            .iter()
            .filter_map(|item| match item {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        Value::Structure(structure) => structure
            .fields()
            .first()
            .map(mime_types_from_value)
            .unwrap_or_default(),
        Value::Variant(inner) => mime_types_from_value(inner),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod owner_change_tests {
    use zbus::Variant as Value;

    use super::mime_types_from_value;

    #[test]
    fn mime_types_as_the_specification_has_them() {
        let value = Value::from(vec!["text/plain", "text/html"]);
        assert_eq!(mime_types_from_value(&value), ["text/plain", "text/html"]);
    }

    #[test]
    fn mime_types_wrapped_in_a_struct_as_gnome_47_sends_them() {
        // A one-field tuple becomes a struct: `(as)`.
        let value = Value::from((vec!["text/plain", "image/png"],));
        assert!(matches!(value, Value::Structure(_)));
        assert_eq!(mime_types_from_value(&value), ["text/plain", "image/png"]);
    }
}
