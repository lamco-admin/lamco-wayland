#![cfg_attr(docsrs, feature(doc_cfg))]

//! XDG Desktop Portal integration for Wayland screen capture and input control
//!
//! This library provides a high-level Rust interface to the XDG Desktop Portal,
//! specifically the ScreenCast, RemoteDesktop, and Clipboard interfaces. It enables
//! applications to capture screen content via PipeWire and inject input events
//! on Wayland compositors.
//!
//! # Features
//!
//! - **Screen capture**: Capture monitor or window content through PipeWire streams
//! - **Input injection**: Send keyboard and mouse events to the desktop
//! - **Clipboard integration**: Portal-based clipboard for remote desktop scenarios
//! - **Multi-monitor support**: Handle multiple displays simultaneously
//! - **Flexible configuration**: Builder pattern and struct literals for Portal options
//! - **Typed errors**: Handle different failure modes appropriately
//!
//! # Requirements
//!
//! This library requires:
//! - A Wayland compositor (e.g., GNOME, KDE Plasma, Sway)
//! - `xdg-desktop-portal` installed and running
//! - A portal backend for your compositor (e.g., `xdg-desktop-portal-gnome`)
//! - PipeWire for video streaming
//!
//! # Quick Start
//!
//! ```no_run
//! use lamco_portal::{PortalManager, PortalConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create portal manager with default config
//! let manager = PortalManager::with_default().await?;
//!
//! // Create a session (triggers permission dialog)
//! let (session, restore_token) = manager.create_session("my-session".to_string(), None).await?;
//!
//! // Access PipeWire file descriptor for video capture
//! let fd = session.pipewire_fd();
//! let streams = session.streams();
//!
//! println!("Capturing {} streams on PipeWire FD {}", streams.len(), fd);
//! # Ok(())
//! # }
//! ```
//!
//! # Configuration
//!
//! Customize Portal behavior using [`PortalConfig`]:
//!
//! ```no_run
//! use lamco_portal::{PortalManager, PortalConfig};
//! use ashpd::desktop::screencast::CursorMode;
//! use ashpd::desktop::PersistMode;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = PortalConfig::builder()
//!     .cursor_mode(CursorMode::Embedded)  // Embed cursor in video
//!     .persist_mode(PersistMode::Application)  // Remember permission
//!     .build();
//!
//! let manager = PortalManager::new(config).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Input Injection
//!
//! Send keyboard and mouse events through the RemoteDesktop portal:
//!
//! ```no_run
//! # use lamco_portal::{PortalManager, PortalConfig};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let manager = PortalManager::with_default().await?;
//! # let (session, _token) = manager.create_session("my-session".to_string(), None).await?;
//! // Move mouse to absolute position
//! manager.remote_desktop()
//!     .notify_pointer_motion_absolute(
//!         session.ashpd_session(),
//!         0,      // stream index
//!         100.0,  // x position
//!         200.0,  // y position
//!     )
//!     .await?;
//!
//! // Click mouse button
//! manager.remote_desktop()
//!     .notify_pointer_button(
//!         session.ashpd_session(),
//!         1,      // button 1 (left)
//!         true,   // pressed
//!     )
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Error Handling
//!
//! The library uses typed errors via [`PortalError`]:
//!
//! ```no_run
//! # use lamco_portal::{PortalManager, PortalConfig, PortalError};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let manager = PortalManager::with_default().await?;
//! match manager.create_session("my-session".to_string(), None).await {
//!     Ok((session, _token)) => {
//!         println!("Session created successfully");
//!     }
//!     Err(PortalError::PermissionDenied) => {
//!         eprintln!("User denied permission in dialog");
//!     }
//!     Err(PortalError::PortalNotAvailable) => {
//!         eprintln!("Portal not installed - install xdg-desktop-portal");
//!     }
//!     Err(e) => {
//!         eprintln!("Other error: {}", e);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Platform Notes
//!
//! - **GNOME**: Works out of the box with `xdg-desktop-portal-gnome`
//! - **KDE Plasma**: Use `xdg-desktop-portal-kde`
//! - **wlroots** (Sway, etc.): Use `xdg-desktop-portal-wlr`
//! - **X11**: Not supported - Wayland only
//!
//! # Security
//!
//! This library triggers system permission dialogs. Users must explicitly grant:
//! - Screen capture access (which monitors/windows to share)
//! - Input injection access (keyboard/mouse control)
//! - Clipboard access (if using clipboard features)
//!
//! Permissions can be remembered per-application using [`ashpd::desktop::PersistMode::Application`].

use std::sync::Arc;
use tracing::{debug, info, trace, warn};

pub mod clipboard;
pub mod config;
pub mod error;
pub mod remote_desktop;
pub mod screencast;
pub mod session;

// Optional ClipboardSink implementation (requires lamco-clipboard-core)
#[cfg(feature = "clipboard-sink")]
pub mod clipboard_sink;

// Optional D-Bus clipboard bridge for GNOME fallback
#[cfg(feature = "dbus-clipboard")]
pub mod dbus_clipboard;

pub use clipboard::ClipboardManager;
pub use config::{PortalConfig, PortalConfigBuilder};
pub use error::{PortalError, Result};
pub use remote_desktop::RemoteDesktopManager;
pub use screencast::ScreenCastManager;

// Re-export ClipboardSink implementation when feature is enabled
#[cfg(feature = "clipboard-sink")]
pub use clipboard_sink::PortalClipboardSink;

// Re-export D-Bus clipboard bridge types when feature is enabled
#[cfg(feature = "dbus-clipboard")]
pub use dbus_clipboard::{DbusClipboardBridge, DbusClipboardEvent};

pub use session::{PortalSessionHandle, SourceType, StreamInfo};

/// Portal manager coordinates all portal interactions
///
/// This is the main entry point for interacting with XDG Desktop Portals.
/// It manages the lifecycle of Portal sessions and provides access to
/// specialized managers for screen capture, input injection, and clipboard.
///
/// # Lifecycle
///
/// 1. Create a `PortalManager` with [`PortalManager::new`] or [`PortalManager::with_default`]
/// 2. Create a session with [`PortalManager::create_session`] (triggers permission dialog)
/// 3. Use the session for screen capture via PipeWire and input injection
/// 4. Clean up with [`PortalManager::cleanup`] when done
///
/// # Examples
///
/// ```no_run
/// use lamco_portal::{PortalManager, PortalConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Simple usage with defaults
/// let manager = PortalManager::with_default().await?;
/// let session = manager.create_session("session-1".to_string(), None).await?;
///
/// // Access specialized managers
/// let screencast = manager.screencast();
/// let remote_desktop = manager.remote_desktop();
/// # Ok(())
/// # }
/// ```
pub struct PortalManager {
    config: PortalConfig,
    #[allow(dead_code)]
    connection: zbus::Connection,
    screencast: Arc<ScreenCastManager>,
    remote_desktop: Arc<RemoteDesktopManager>,
    clipboard: Option<Arc<ClipboardManager>>,
}

impl PortalManager {
    /// Create new portal manager with specified configuration
    ///
    /// # Examples
    ///
    /// With defaults:
    /// ```no_run
    /// # use lamco_portal::{PortalManager, PortalConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let manager = PortalManager::new(PortalConfig::default()).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// With custom config:
    /// ```no_run
    /// # use lamco_portal::{PortalManager, PortalConfig};
    /// # use ashpd::desktop::screencast::CursorMode;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = PortalConfig {
    ///     cursor_mode: CursorMode::Embedded,
    ///     ..Default::default()
    /// };
    /// let manager = PortalManager::new(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(config: PortalConfig) -> Result<Self> {
        info!("Initializing Portal Manager");

        // Connect to session D-Bus
        let connection = zbus::Connection::session().await?;

        // Log connection details for debugging session issues
        if let Some(unique_name) = connection.unique_name() {
            debug!(
                dbus_unique_name = %unique_name,
                "Connected to D-Bus session bus"
            );
        } else {
            debug!("Connected to D-Bus session bus (no unique name yet)");
        }

        let screencast = Arc::new(ScreenCastManager::new(connection.clone(), &config).await?);

        let remote_desktop =
            Arc::new(RemoteDesktopManager::new(connection.clone(), &config).await?);

        // Clipboard manager requires a RemoteDesktop session
        // It will be created after session is established in create_session_with_clipboard()

        info!("Portal Manager initialized successfully");

        Ok(Self {
            config,
            connection,
            screencast,
            remote_desktop,
            clipboard: None, // Created later with session
        })
    }

    /// Create new portal manager with default configuration
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use lamco_portal::PortalManager;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let manager = PortalManager::with_default().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn with_default() -> Result<Self> {
        Self::new(PortalConfig::default()).await
    }

    /// Create a complete portal session (ScreenCast for video, RemoteDesktop for input, optionally Clipboard)
    ///
    /// This triggers the user permission dialog and returns a session handle
    /// with PipeWire access for video and input injection capabilities.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Unique identifier for this session (user-provided)
    /// * `clipboard` - Optional Clipboard manager to enable for this session
    ///
    /// # Flow
    ///
    /// 1. Create combined RemoteDesktop session (includes ScreenCast capability)
    /// 2. Select devices (keyboard + pointer for input injection)
    /// 3. Select sources (monitors to capture for screen sharing)
    /// 4. Request clipboard access (if clipboard provided) ← BEFORE START
    /// 5. Start session (triggers permission dialog, unless restore token valid)
    /// 6. Get PipeWire FD, stream information, and restore token
    ///
    /// # Returns
    ///
    /// Tuple of (PortalSessionHandle, Optional restore token)
    ///
    /// The restore token should be stored securely and passed in the next
    /// session's PortalConfig to avoid permission dialogs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use lamco_portal::{PortalManager, PortalConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let manager = PortalManager::new(PortalConfig::default()).await?;
    /// let session = manager.create_session("my-session-1".to_string(), None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_session(
        &self,
        session_id: String,
        clipboard: Option<&crate::clipboard::ClipboardManager>,
    ) -> Result<(PortalSessionHandle, Option<String>)> {
        info!("Creating combined portal session (ScreenCast + RemoteDesktop)");

        // RemoteDesktop session type supports both input injection and screen sharing
        let remote_desktop_session =
            self.remote_desktop.create_session().await.map_err(|e| {
                PortalError::session_creation(format!("RemoteDesktop session: {}", e))
            })?;

        // Log session creation for clipboard debugging
        // Note: ashpd Session.path() is private, so we generate our own tracking ID
        let session_tracking_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        info!(
            session_id = %session_tracking_id,
            "RemoteDesktop session created"
        );
        trace!(
            session_id = %session_tracking_id,
            "Session state is INIT (required for clipboard.request)"
        );

        // Portal spec requires clipboard.request() when session state is INIT,
        // which means we must call it before SelectDevices or SelectSources.
        if let Some(clipboard_mgr) = clipboard {
            debug!(session_id = %session_tracking_id, "Requesting clipboard access");

            match clipboard_mgr
                .portal_clipboard()
                .request(&remote_desktop_session)
                .await
            {
                Ok(()) => {
                    info!(session_id = %session_tracking_id, "Clipboard enabled for session");
                }
                Err(e) => {
                    warn!(
                        session_id = %session_tracking_id,
                        error = %e,
                        "Clipboard request failed"
                    );
                    if format!("{}", e).contains("Invalid state") {
                        warn!("Portal daemon may have stale session state - clipboard unavailable");
                    }
                }
            }
        }

        // Select devices for input injection (from config)
        // Must close session on any error to prevent orphaned D-Bus state.
        // ashpd's Session does NOT implement Drop with Close() - we must do it explicitly.
        if let Err(e) = self
            .remote_desktop
            .select_devices(&remote_desktop_session, self.config.devices)
            .await
        {
            warn!("Device selection failed, closing session: {}", e);
            let _ = remote_desktop_session.close().await;
            return Err(PortalError::session_creation(format!(
                "Device selection: {}",
                e
            )));
        }

        info!("Input devices selected from config");

        // ScreenCast is required to make screen sources available for sharing
        let screencast_proxy = ashpd::desktop::screencast::Screencast::new().await?;

        if let Err(e) = screencast_proxy
            .select_sources(
                &remote_desktop_session,
                self.config.cursor_mode,
                self.config.source_type,
                self.config.allow_multiple,
                self.config.restore_token.as_deref(),
                self.config.persist_mode,
            )
            .await
        {
            // Close session before returning to prevent GNOME Shell from tracking stale state.
            // Without cleanup, retry attempts fail with "Invalid state" from portal daemon.
            warn!("Source selection failed, closing session: {}", e);
            let _ = remote_desktop_session.close().await;
            return Err(PortalError::session_creation(format!(
                "Source selection: {}",
                e
            )));
        }

        info!("Screen sources selected - permission dialog will appear");

        // Start the combined session (triggers permission dialog, unless restore token valid)
        // Note: clipboard.request() was already called earlier, immediately after CreateSession
        let (pipewire_fd, streams, restore_token) = match self
            .remote_desktop
            .start_session(&remote_desktop_session)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!("Session start failed, closing session: {}", e);
                let _ = remote_desktop_session.close().await;
                return Err(PortalError::session_creation(format!(
                    "Session start: {}",
                    e
                )));
            }
        };

        info!("Portal session started successfully");
        info!("  PipeWire FD: {:?}", pipewire_fd);
        info!("  Streams: {}", streams.len());

        if let Some(ref token) = restore_token {
            info!("  Restore Token: Received ({} chars)", token.len());
        } else {
            debug!("  Restore Token: None (portal may not support persistence)");
        }

        if streams.is_empty() {
            warn!("No streams available, closing session");
            let _ = remote_desktop_session.close().await;
            return Err(PortalError::NoStreamsAvailable);
        }

        // Keep session alive for input injection to remain functional
        let stream_count = streams.len();
        let handle = PortalSessionHandle::new(
            session_id.clone(),
            pipewire_fd,
            streams,
            Some(session_id.clone()), // Store session ID for input operations
            remote_desktop_session,   // Pass the actual ashpd session for input injection
        );

        info!(
            "Portal session handle created with {} streams",
            stream_count
        );

        Ok((handle, restore_token))
    }

    /// Access the ScreenCast manager
    ///
    /// Use this to access ScreenCast-specific functionality if needed.
    /// Most users will use [`PortalManager::create_session`] instead.
    pub fn screencast(&self) -> &Arc<ScreenCastManager> {
        &self.screencast
    }

    /// Access the RemoteDesktop manager
    ///
    /// Use this to inject input events (keyboard, mouse, scroll) into
    /// the desktop session. Requires an active session from
    /// [`PortalManager::create_session`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use lamco_portal::PortalManager;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let manager = PortalManager::with_default().await?;
    /// # let (session, _token) = manager.create_session("s1".to_string(), None).await?;
    /// // Inject mouse movement
    /// manager.remote_desktop()
    ///     .notify_pointer_motion_absolute(
    ///         session.ashpd_session(),
    ///         0, 100.0, 200.0
    ///     )
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn remote_desktop(&self) -> &Arc<RemoteDesktopManager> {
        &self.remote_desktop
    }

    /// Access the Clipboard manager if available
    ///
    /// Returns `None` if no clipboard manager has been set.
    /// Clipboard integration is optional and must be explicitly enabled.
    pub fn clipboard(&self) -> Option<&Arc<ClipboardManager>> {
        self.clipboard.as_ref()
    }

    /// Set clipboard manager (called after session creation)
    ///
    /// This is typically used internally during session setup.
    /// Most users should not need to call this directly.
    pub fn set_clipboard(&mut self, clipboard: Arc<ClipboardManager>) {
        self.clipboard = Some(clipboard);
    }

    /// Cleanup all portal resources
    ///
    /// Portal sessions are automatically cleaned up when dropped,
    /// so calling this explicitly is optional. It can be useful for
    /// logging cleanup or performing graceful shutdown.
    pub async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up portal resources");
        // Portal sessions are automatically cleaned up when dropped
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Wayland session
    async fn test_portal_manager_creation() {
        let config = PortalConfig::default();
        let manager = PortalManager::new(config).await;

        // May fail if not in Wayland session or portal not available
        if manager.is_err() {
            eprintln!("Portal manager creation failed (expected if not in Wayland session)");
        }
    }

    #[tokio::test]
    #[ignore] // Requires Wayland session
    async fn test_portal_manager_with_default() {
        let manager = PortalManager::with_default().await;

        // May fail if not in Wayland session or portal not available
        if manager.is_err() {
            eprintln!("Portal manager creation failed (expected if not in Wayland session)");
        }
    }
}
