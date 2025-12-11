//! Error types for Portal operations
//!
//! Provides typed errors that library users can match and handle specifically.

use thiserror::Error;

/// Errors that can occur during Portal operations
///
/// All Portal operations return `Result<T, PortalError>`, allowing users to
/// handle different error cases appropriately.
///
/// # Examples
///
/// ```no_run
/// # use lamco_portal::{PortalManager, PortalConfig, PortalError};
/// # async fn example() -> Result<(), PortalError> {
/// let manager = PortalManager::new(PortalConfig::default()).await?;
///
/// match manager.create_session("session-1".to_string(), None).await {
///     Ok(session) => {
///         println!("Session created successfully");
///     }
///     Err(PortalError::PermissionDenied) => {
///         eprintln!("User denied permission");
///     }
///     Err(PortalError::PortalNotAvailable) => {
///         eprintln!("Portal not installed or not running");
///     }
///     Err(e) => {
///         eprintln!("Other error: {}", e);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Error, Debug)]
pub enum PortalError {
    /// Failed to connect to D-Bus session bus
    ///
    /// This usually indicates D-Bus is not running or not accessible.
    /// Common on systems without a desktop session.
    #[error("Failed to connect to D-Bus session bus")]
    DbusConnection(#[from] zbus::Error),

    /// Portal request failed
    ///
    /// This covers all Portal-specific errors from ashpd, including:
    /// - User denied permission
    /// - Portal not available
    /// - Invalid request parameters
    #[error("Portal request failed: {0}")]
    PortalRequest(#[from] ashpd::Error),

    /// User denied the Portal permission request
    ///
    /// The user explicitly denied permission in the system dialog.
    /// The application should handle this gracefully.
    #[error("User denied permission")]
    PermissionDenied,

    /// Portal is not available on this system
    ///
    /// This can occur when:
    /// - xdg-desktop-portal is not installed
    /// - No portal backend is running (e.g., xdg-desktop-portal-gnome)
    /// - Not running in a Wayland session
    #[error("Portal not available - check xdg-desktop-portal installation")]
    PortalNotAvailable,

    /// Session creation failed
    ///
    /// Failed to create a Portal session. This may be due to:
    /// - Portal not responding
    /// - Invalid configuration
    /// - System limitations
    #[error("Session creation failed: {0}")]
    SessionCreation(String),

    /// No streams available after session start
    ///
    /// This occurs when the session starts successfully but no PipeWire
    /// streams are provided. Usually indicates user denied screen access
    /// or no screens/windows are available to share.
    #[error("No streams available - user may have denied screen access")]
    NoStreamsAvailable,

    /// Failed to open PipeWire connection
    ///
    /// The Portal session started but we couldn't get the PipeWire file
    /// descriptor for accessing the stream.
    #[error("Failed to open PipeWire connection: {0}")]
    PipeWireFailed(String),

    /// Input injection failed
    ///
    /// Failed to inject keyboard or pointer input through the Portal.
    /// This may occur if input permission wasn't granted or the session
    /// is no longer valid.
    #[error("Input injection failed: {0}")]
    InputInjectionFailed(String),

    /// Clipboard operation failed
    ///
    /// Failed to perform a clipboard operation through the Portal.
    #[error("Clipboard operation failed: {0}")]
    ClipboardFailed(String),

    /// I/O operation failed
    ///
    /// File descriptor or pipe operation failed during clipboard data transfer.
    #[error("I/O operation failed: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid configuration
    ///
    /// The provided configuration is invalid or incompatible.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Result type for Portal operations
///
/// This is a convenience alias for `Result<T, PortalError>`.
pub type Result<T> = std::result::Result<T, PortalError>;

// Helper implementations for common error patterns
impl PortalError {
    /// Create a session creation error
    pub(crate) fn session_creation(msg: impl Into<String>) -> Self {
        Self::SessionCreation(msg.into())
    }

    /// Create a PipeWire error
    #[allow(dead_code)]
    pub(crate) fn pipewire_failed(msg: impl Into<String>) -> Self {
        Self::PipeWireFailed(msg.into())
    }

    /// Create an input injection error
    pub(crate) fn input_injection(msg: impl Into<String>) -> Self {
        Self::InputInjectionFailed(msg.into())
    }

    /// Create a clipboard error
    #[allow(dead_code)]
    pub(crate) fn clipboard(msg: impl Into<String>) -> Self {
        Self::ClipboardFailed(msg.into())
    }

    /// Create an invalid config error
    #[allow(dead_code)]
    pub(crate) fn invalid_config(msg: impl Into<String>) -> Self {
        Self::InvalidConfig(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PortalError::PermissionDenied;
        assert_eq!(err.to_string(), "User denied permission");

        let err = PortalError::session_creation("test reason");
        assert_eq!(err.to_string(), "Session creation failed: test reason");
    }

    #[test]
    fn test_error_helpers() {
        let err = PortalError::pipewire_failed("connection lost");
        assert!(matches!(err, PortalError::PipeWireFailed(_)));

        let err = PortalError::input_injection("invalid keycode");
        assert!(matches!(err, PortalError::InputInjectionFailed(_)));
    }
}
