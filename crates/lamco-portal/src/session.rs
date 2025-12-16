//! Portal session management
//!
//! Manages the lifecycle of portal sessions and associated resources.

use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use tracing::info;

/// Information about a PipeWire stream from the portal
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// PipeWire node ID
    pub node_id: u32,

    /// Stream position (for multi-monitor)
    pub position: (i32, i32),

    /// Stream size
    pub size: (u32, u32),

    /// Source type (monitor, window, etc.)
    pub source_type: SourceType,
}

/// Source type for streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Monitor,
    Window,
    Virtual,
}

/// Handle to an active portal session
///
/// This represents a running Portal session with screen capture and input
/// injection capabilities. It provides access to:
/// - PipeWire file descriptor for video stream capture
/// - Stream information (one per monitor/window)
/// - The underlying ashpd session for input injection
///
/// # Lifecycle
///
/// Created by [`PortalManager::create_session`]. The session remains active
/// until this handle is dropped. Dropping the handle will automatically close
/// the Portal session and stop all streams.
///
/// # Examples
///
/// ```no_run
/// # use lamco_portal::PortalManager;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let manager = PortalManager::with_default().await?;
/// let session = manager.create_session("my-session".to_string(), None).await?;
///
/// // Access PipeWire FD for video capture
/// let fd = session.pipewire_fd();
/// println!("PipeWire FD: {}", fd);
///
/// // Get stream information
/// for stream in session.streams() {
///     println!("Stream {}: {}x{} at ({}, {})",
///         stream.node_id,
///         stream.size.0, stream.size.1,
///         stream.position.0, stream.position.1
///     );
/// }
///
/// // Use for input injection
/// manager.remote_desktop()
///     .notify_pointer_button(session.ashpd_session(), 1, true)
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct PortalSessionHandle {
    /// Session identifier from portal
    pub session_id: String,

    /// PipeWire file descriptor (owned - will be closed on drop)
    pipewire_fd: OwnedFd,

    /// Available streams (one per monitor typically)
    pub streams: Vec<StreamInfo>,

    /// RemoteDesktop session for input injection
    pub remote_desktop_session: Option<String>,

    /// Active ashpd session (needed for input injection)
    pub session: ashpd::desktop::Session<'static, ashpd::desktop::remote_desktop::RemoteDesktop<'static>>,
}

impl PortalSessionHandle {
    /// Create new session handle
    pub fn new(
        session_id: String,
        pipewire_fd: OwnedFd,
        streams: Vec<StreamInfo>,
        remote_desktop_session: Option<String>,
        session: ashpd::desktop::Session<'static, ashpd::desktop::remote_desktop::RemoteDesktop<'static>>,
    ) -> Self {
        info!(
            "Created portal session handle: {}, {} streams, fd: {:?}",
            session_id,
            streams.len(),
            pipewire_fd
        );

        Self {
            session_id,
            pipewire_fd,
            streams,
            remote_desktop_session,
            session,
        }
    }

    /// Get PipeWire file descriptor as raw fd
    ///
    /// Returns the raw file descriptor for use with PipeWire. The fd remains
    /// owned by this handle and will be closed when the handle is dropped.
    pub fn pipewire_fd(&self) -> RawFd {
        self.pipewire_fd.as_raw_fd()
    }

    /// Get stream information
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get remote desktop session (for input injection)
    pub fn remote_desktop_session(&self) -> Option<&str> {
        self.remote_desktop_session.as_deref()
    }

    /// Get reference to the underlying ashpd session
    ///
    /// Required for input injection operations via [`RemoteDesktopManager`].
    /// Most operations that need this will accept `session.ashpd_session()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use lamco_portal::PortalManager;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let manager = PortalManager::with_default().await?;
    /// # let session = manager.create_session("s1".to_string(), None).await?;
    /// // Inject input using the ashpd session
    /// manager.remote_desktop()
    ///     .notify_pointer_button(session.ashpd_session(), 1, true)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn ashpd_session(
        &self,
    ) -> &ashpd::desktop::Session<'static, ashpd::desktop::remote_desktop::RemoteDesktop<'static>> {
        &self.session
    }

    /// Explicitly close the portal session
    ///
    /// This consumes the handle and closes all resources. The same effect can
    /// be achieved by simply dropping the handle, but this method provides
    /// explicit logging.
    pub fn close(self) {
        info!("Closing portal session: {}", self.session_id);
        // OwnedFd and Session are automatically closed on drop
        drop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_info_creation() {
        let stream = StreamInfo {
            node_id: 42,
            position: (0, 0),
            size: (1920, 1080),
            source_type: SourceType::Monitor,
        };

        assert_eq!(stream.node_id, 42);
        assert_eq!(stream.position, (0, 0));
        assert_eq!(stream.size, (1920, 1080));
        assert!(matches!(stream.source_type, SourceType::Monitor));
    }

    #[test]
    fn test_source_type_variants() {
        assert!(matches!(SourceType::Monitor, SourceType::Monitor));
        assert!(matches!(SourceType::Window, SourceType::Window));
        assert!(matches!(SourceType::Virtual, SourceType::Virtual));
    }

    // Note: PortalSessionHandle::new() requires an actual ashpd::Session which
    // can only be created with a D-Bus connection. Integration tests for session
    // creation are marked with #[ignore] and require a running Wayland session.
}
