//! D-Bus clipboard bridge for GNOME fallback.
//!
//! This module provides a workaround for GNOME where the XDG Desktop Portal's
//! `SelectionOwnerChanged` signal is not reliably emitted. It connects to the
//! `org.wayland_rdp.Clipboard` D-Bus interface provided by the `wayland-rdp-clipboard`
//! GNOME Shell extension.
//!
//! # Feature Flag
//!
//! This module requires the `dbus-clipboard` feature:
//!
//! ```toml
//! [dependencies]
//! lamco-portal = { version = "0.1", features = ["dbus-clipboard"] }
//! ```
//!
//! # D-Bus Interface
//!
//! The bridge listens to the following D-Bus interface:
//!
//! - **Service**: `org.wayland_rdp.Clipboard`
//! - **Path**: `/org/wayland_rdp/Clipboard`
//! - **Interface**: `org.wayland_rdp.Clipboard`
//! - **Signal**: `ClipboardChanged(mime_types: Vec<String>, content_hash: String)`
//!
//! # Example
//!
//! ```ignore
//! use lamco_portal::dbus_clipboard::DbusClipboardBridge;
//!
//! let bridge = DbusClipboardBridge::connect().await?;
//! let mut receiver = bridge.subscribe();
//!
//! while let Some(event) = receiver.recv().await {
//!     println!("Clipboard changed: {:?}", event.mime_types);
//! }
//! ```

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use zbus::{proxy, Connection};

use crate::error::PortalError;

/// Well-known D-Bus service name for the clipboard extension.
pub const DBUS_SERVICE: &str = "org.wayland_rdp.Clipboard";
/// D-Bus object path for the clipboard interface.
pub const DBUS_PATH: &str = "/org/wayland_rdp/Clipboard";
/// D-Bus interface name for clipboard operations.
pub const DBUS_INTERFACE: &str = "org.wayland_rdp.Clipboard";

/// Event emitted when the clipboard content changes via D-Bus.
#[derive(Debug, Clone)]
pub struct DbusClipboardEvent {
    /// MIME types available in the clipboard.
    pub mime_types: Vec<String>,
    /// Hash of the clipboard content (for deduplication).
    pub content_hash: String,
}

/// D-Bus proxy for the wayland-rdp-clipboard GNOME Shell extension.
#[proxy(
    interface = "org.wayland_rdp.Clipboard",
    default_service = "org.wayland_rdp.Clipboard",
    default_path = "/org/wayland_rdp/Clipboard"
)]
trait WaylandRdpClipboard {
    /// Signal emitted when clipboard content changes.
    #[zbus(signal)]
    fn clipboard_changed(&self, mime_types: Vec<String>, content_hash: String);

    /// Get the current clipboard MIME types.
    fn get_mime_types(&self) -> zbus::Result<Vec<String>>;
}

/// D-Bus clipboard bridge for GNOME fallback.
///
/// This bridge connects to the `org.wayland_rdp.Clipboard` D-Bus service
/// provided by the GNOME Shell extension and forwards clipboard change
/// events to subscribers.
pub struct DbusClipboardBridge {
    _connection: Arc<Connection>,
    sender: broadcast::Sender<DbusClipboardEvent>,
}

impl DbusClipboardBridge {
    /// Connect to the D-Bus clipboard service.
    ///
    /// Returns an error if the D-Bus connection fails or if the
    /// clipboard service is not available.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let bridge = DbusClipboardBridge::connect().await?;
    /// ```
    pub async fn connect() -> Result<Self, PortalError> {
        let connection = Connection::session()
            .await
            .map_err(|e| PortalError::session_creation(format!("D-Bus connection failed: {}", e)))?;

        let connection = Arc::new(connection);
        let (sender, _) = broadcast::channel(64);

        let bridge = Self {
            _connection: connection.clone(),
            sender,
        };

        // Spawn the signal listener task
        let sender_clone = bridge.sender.clone();
        let conn_clone = connection.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::listen_for_signals(conn_clone, sender_clone).await {
                error!("D-Bus clipboard listener error: {}", e);
            }
        });

        info!("D-Bus clipboard bridge connected");
        Ok(bridge)
    }

    /// Subscribe to clipboard change events.
    ///
    /// Returns a broadcast receiver that will receive events whenever
    /// the clipboard content changes.
    pub fn subscribe(&self) -> broadcast::Receiver<DbusClipboardEvent> {
        self.sender.subscribe()
    }

    /// Check if the D-Bus clipboard service is available.
    ///
    /// This performs a name lookup on the session bus to verify that
    /// the `org.wayland_rdp.Clipboard` service is registered.
    pub async fn is_available() -> bool {
        let Ok(conn) = Connection::session().await else {
            return false;
        };

        let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
            return false;
        };

        // Use the service name directly - the proxy handles conversion
        dbus.name_has_owner(DBUS_SERVICE.try_into().expect("valid bus name"))
            .await
            .unwrap_or(false)
    }

    /// Get the current clipboard MIME types from the D-Bus service.
    ///
    /// Returns `None` if the service is not available or an error occurs.
    pub async fn get_current_mime_types(connection: &Connection) -> Option<Vec<String>> {
        let proxy = WaylandRdpClipboardProxy::new(connection).await.ok()?;
        proxy.get_mime_types().await.ok()
    }

    /// Internal: Listen for clipboard change signals.
    async fn listen_for_signals(
        connection: Arc<Connection>,
        sender: broadcast::Sender<DbusClipboardEvent>,
    ) -> Result<(), PortalError> {
        let proxy = WaylandRdpClipboardProxy::new(&connection)
            .await
            .map_err(|e| PortalError::session_creation(format!("Failed to create proxy: {}", e)))?;

        let mut stream = proxy
            .receive_clipboard_changed()
            .await
            .map_err(|e| PortalError::session_creation(format!("Failed to subscribe to signal: {}", e)))?;

        debug!("Listening for D-Bus clipboard signals");

        use futures_util::StreamExt;
        while let Some(signal) = stream.next().await {
            match signal.args() {
                Ok(args) => {
                    let event = DbusClipboardEvent {
                        mime_types: args.mime_types.clone(),
                        content_hash: args.content_hash.clone(),
                    };

                    let hash_preview = if event.content_hash.len() > 16 {
                        &event.content_hash[..16]
                    } else {
                        &event.content_hash
                    };

                    debug!(
                        "D-Bus clipboard change: {} MIME types, hash={}",
                        event.mime_types.len(),
                        hash_preview
                    );

                    // Send to subscribers (ignore errors if no receivers)
                    let _ = sender.send(event);
                }
                Err(e) => {
                    warn!("Failed to parse clipboard signal args: {}", e);
                }
            }
        }

        warn!("D-Bus clipboard signal stream ended");
        Ok(())
    }
}

/// Builder for configuring the D-Bus clipboard bridge.
pub struct DbusClipboardBridgeBuilder {
    channel_capacity: usize,
}

impl Default for DbusClipboardBridgeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DbusClipboardBridgeBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self { channel_capacity: 64 }
    }

    /// Set the broadcast channel capacity.
    ///
    /// Higher values allow more buffered events but use more memory.
    /// Default is 64.
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Build and connect the D-Bus clipboard bridge.
    pub async fn build(self) -> Result<DbusClipboardBridge, PortalError> {
        let connection = Connection::session()
            .await
            .map_err(|e| PortalError::session_creation(format!("D-Bus connection failed: {}", e)))?;

        let connection = Arc::new(connection);
        let (sender, _) = broadcast::channel(self.channel_capacity);

        let bridge = DbusClipboardBridge {
            _connection: connection.clone(),
            sender,
        };

        // Spawn the signal listener task
        let sender_clone = bridge.sender.clone();
        let conn_clone = connection.clone();
        tokio::spawn(async move {
            if let Err(e) = DbusClipboardBridge::listen_for_signals(conn_clone, sender_clone).await {
                error!("D-Bus clipboard listener error: {}", e);
            }
        });

        info!("D-Bus clipboard bridge connected (capacity={})", self.channel_capacity);
        Ok(bridge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(DBUS_SERVICE, "org.wayland_rdp.Clipboard");
        assert_eq!(DBUS_PATH, "/org/wayland_rdp/Clipboard");
        assert_eq!(DBUS_INTERFACE, "org.wayland_rdp.Clipboard");
    }

    #[test]
    fn test_event_clone() {
        let event = DbusClipboardEvent {
            mime_types: vec!["text/plain".to_string()],
            content_hash: "abc123".to_string(),
        };
        let cloned = event.clone();
        assert_eq!(cloned.mime_types, event.mime_types);
        assert_eq!(cloned.content_hash, event.content_hash);
    }

    #[test]
    fn test_builder_default() {
        let builder = DbusClipboardBridgeBuilder::default();
        assert_eq!(builder.channel_capacity, 64);
    }

    #[test]
    fn test_builder_capacity() {
        let builder = DbusClipboardBridgeBuilder::new().channel_capacity(128);
        assert_eq!(builder.channel_capacity, 128);
    }
}
