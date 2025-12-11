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
//! let session = manager.create_session("my-session".to_string(), None).await?;
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
//! # let session = manager.create_session("my-session".to_string(), None).await?;
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
//!     Ok(session) => {
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
//! Permissions can be remembered per-application using [`PersistMode::Application`].

use std::sync::Arc;
use tracing::{debug, info, warn};

pub mod clipboard;
pub mod config;
pub mod error;
pub mod remote_desktop;
pub mod screencast;
pub mod session;

pub use clipboard::ClipboardManager;
pub use config::{PortalConfig, PortalConfigBuilder};
pub use error::{PortalError, Result};
pub use remote_desktop::RemoteDesktopManager;
pub use screencast::ScreenCastManager;
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

        debug!("Connected to D-Bus session bus");

        // Initialize portal managers
        let screencast = Arc::new(ScreenCastManager::new(connection.clone(), &config).await?);

        let remote_desktop = Arc::new(RemoteDesktopManager::new(connection.clone(), &config).await?);

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
    /// 5. Start session (triggers permission dialog)
    /// 6. Get PipeWire FD and stream information
    ///
    /// # Returns
    ///
    /// PortalSessionHandle with PipeWire FD, stream information, and session reference
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
    ) -> Result<PortalSessionHandle> {
        info!("Creating combined portal session (ScreenCast + RemoteDesktop)");

        // Create RemoteDesktop session (this type of session can include screen sharing)
        let remote_desktop_session = self
            .remote_desktop
            .create_session()
            .await
            .map_err(|e| PortalError::session_creation(format!("RemoteDesktop session: {}", e)))?;

        info!("RemoteDesktop session created");

        // Select devices for input injection (from config)
        self.remote_desktop
            .select_devices(&remote_desktop_session, self.config.devices)
            .await
            .map_err(|e| PortalError::session_creation(format!("Device selection: {}", e)))?;

        info!("Input devices selected from config");

        // CRITICAL FIX: Also use ScreenCast to select screen sources
        // This is what makes screens available for sharing
        let screencast_proxy = ashpd::desktop::screencast::Screencast::new().await?;

        screencast_proxy
            .select_sources(
                &remote_desktop_session,              // Use same session
                self.config.cursor_mode,              // From config
                self.config.source_type,              // From config (already BitFlags)
                self.config.allow_multiple,           // From config
                self.config.restore_token.as_deref(), // From config
                self.config.persist_mode,             // From config
            )
            .await
            .map_err(|e| PortalError::session_creation(format!("Source selection: {}", e)))?;

        info!("Screen sources selected - permission dialog will appear");

        // Request clipboard access BEFORE starting session (required by Portal spec)
        if let Some(clipboard_mgr) = clipboard {
            info!("Requesting clipboard access for session");
            if let Err(e) = clipboard_mgr.portal_clipboard().request(&remote_desktop_session).await {
                warn!("Failed to request clipboard access: {}", e);
                warn!("Clipboard will not be available");
            } else {
                info!("Clipboard access requested for session");
            }
        }

        // Start the combined session (triggers permission dialog)
        let (pipewire_fd, streams) = self
            .remote_desktop
            .start_session(&remote_desktop_session)
            .await
            .map_err(|e| PortalError::session_creation(format!("Session start: {}", e)))?;

        info!("Portal session started successfully");
        info!("  PipeWire FD: {}", pipewire_fd);
        info!("  Streams: {}", streams.len());

        if streams.is_empty() {
            return Err(PortalError::NoStreamsAvailable);
        }

        // Create session handle with session reference
        // We need to keep the session alive for input injection
        let stream_count = streams.len();
        let handle = PortalSessionHandle::new(
            session_id.clone(),
            pipewire_fd,
            streams,
            Some(session_id.clone()), // Store session ID for input operations
            remote_desktop_session,   // Pass the actual ashpd session for input injection
        );

        info!("Portal session handle created with {} streams", stream_count);

        Ok(handle)
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
    /// # let session = manager.create_session("s1".to_string(), None).await?;
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
