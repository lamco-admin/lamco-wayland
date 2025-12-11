//! Configuration types for Portal operations
//!
//! Provides flexible configuration for Portal sessions through both struct literals
//! and builder patterns.

use ashpd::desktop::remote_desktop::DeviceType;
use ashpd::desktop::screencast::{CursorMode, SourceType};
use ashpd::desktop::PersistMode;
use enumflags2::BitFlags;

/// Configuration for Portal session behavior
///
/// Controls how Portal requests are made and what capabilities are requested.
/// All fields have sensible defaults suitable for screen capture and input control.
///
/// # Examples
///
/// Using defaults:
/// ```no_run
/// # use lamco_portal::PortalConfig;
/// let config = PortalConfig::default();
/// ```
///
/// Using struct literal with defaults:
/// ```no_run
/// # use lamco_portal::{PortalConfig};
/// # use ashpd::desktop::screencast::CursorMode;
/// let config = PortalConfig {
///     cursor_mode: CursorMode::Embedded,
///     ..Default::default()
/// };
/// ```
///
/// Using builder:
/// ```no_run
/// # use lamco_portal::PortalConfig;
/// # use ashpd::desktop::screencast::CursorMode;
/// # use ashpd::desktop::PersistMode;
/// let config = PortalConfig::builder()
///     .cursor_mode(CursorMode::Embedded)
///     .persist_mode(PersistMode::Application)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PortalConfig {
    /// How cursor should be handled in screen capture
    ///
    /// - `Hidden`: Cursor not visible in stream
    /// - `Embedded`: Cursor baked into video stream
    /// - `Metadata`: Cursor position provided as metadata (recommended for remote desktop)
    pub cursor_mode: CursorMode,

    /// Whether to persist session permissions
    ///
    /// - `DoNot`: Request permission every time (most secure)
    /// - `Application`: Remember permission for this app (skip dialog on reconnect)
    /// - `ExplicitlyRevoked`: Remember until user explicitly revokes
    pub persist_mode: PersistMode,

    /// What types of sources can be captured
    ///
    /// Can be combined: `SourceType::Monitor | SourceType::Window`
    /// - `Monitor`: Physical monitors
    /// - `Window`: Individual windows
    /// - `Virtual`: Virtual sources (uncommon)
    pub source_type: BitFlags<SourceType>,

    /// What input devices to enable for injection
    ///
    /// Can be combined: `DeviceType::Keyboard | DeviceType::Pointer`
    /// - `Keyboard`: Keyboard input injection
    /// - `Pointer`: Mouse/pointer input injection
    /// - `Touchscreen`: Touch input injection (less common)
    pub devices: BitFlags<DeviceType>,

    /// Allow selecting multiple sources (monitors/windows)
    ///
    /// Most screen sharing scenarios want `true` to support multi-monitor setups
    pub allow_multiple: bool,

    /// Restore token from previous session
    ///
    /// If provided and session was persisted, can skip permission dialog.
    /// Obtain from previous session via Portal response (advanced usage).
    pub restore_token: Option<String>,
}

impl Default for PortalConfig {
    /// Create configuration with sensible defaults
    ///
    /// - Cursor as metadata (best for remote desktop)
    /// - No persistence (request permission each time)
    /// - Both monitors and windows available
    /// - Keyboard + pointer input enabled
    /// - Multiple sources allowed
    /// - No restore token
    fn default() -> Self {
        Self {
            cursor_mode: CursorMode::Metadata,
            persist_mode: PersistMode::DoNot,
            source_type: SourceType::Monitor | SourceType::Window,
            devices: DeviceType::Keyboard | DeviceType::Pointer,
            allow_multiple: true,
            restore_token: None,
        }
    }
}

impl PortalConfig {
    /// Create a new builder for PortalConfig
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use lamco_portal::PortalConfig;
    /// # use ashpd::desktop::screencast::CursorMode;
    /// let config = PortalConfig::builder()
    ///     .cursor_mode(CursorMode::Hidden)
    ///     .build();
    /// ```
    pub fn builder() -> PortalConfigBuilder {
        PortalConfigBuilder::default()
    }
}

/// Builder for PortalConfig
///
/// Provides a fluent API for configuring Portal behavior.
/// All fields are optional and will use sensible defaults if not specified.
///
/// # Examples
///
/// ```no_run
/// # use lamco_portal::PortalConfig;
/// # use ashpd::desktop::screencast::{CursorMode, SourceType};
/// # use ashpd::desktop::PersistMode;
/// # use ashpd::desktop::remote_desktop::DeviceType;
/// let config = PortalConfig::builder()
///     .cursor_mode(CursorMode::Embedded)
///     .persist_mode(PersistMode::Application)
///     .source_type(SourceType::Monitor.into())
///     .devices(DeviceType::Keyboard.into())
///     .allow_multiple(false)
///     .build();
/// ```
#[derive(Default, Debug)]
pub struct PortalConfigBuilder {
    cursor_mode: Option<CursorMode>,
    persist_mode: Option<PersistMode>,
    source_type: Option<BitFlags<SourceType>>,
    devices: Option<BitFlags<DeviceType>>,
    allow_multiple: Option<bool>,
    restore_token: Option<String>,
}

impl PortalConfigBuilder {
    /// Set cursor mode for screen capture
    ///
    /// Default: `CursorMode::Metadata`
    pub fn cursor_mode(mut self, mode: CursorMode) -> Self {
        self.cursor_mode = Some(mode);
        self
    }

    /// Set session persistence mode
    ///
    /// Default: `PersistMode::DoNot`
    pub fn persist_mode(mut self, mode: PersistMode) -> Self {
        self.persist_mode = Some(mode);
        self
    }

    /// Set source types that can be captured
    ///
    /// Default: `SourceType::Monitor | SourceType::Window`
    pub fn source_type(mut self, types: BitFlags<SourceType>) -> Self {
        self.source_type = Some(types);
        self
    }

    /// Set input device types to enable
    ///
    /// Default: `DeviceType::Keyboard | DeviceType::Pointer`
    pub fn devices(mut self, devices: BitFlags<DeviceType>) -> Self {
        self.devices = Some(devices);
        self
    }

    /// Set whether multiple sources can be selected
    ///
    /// Default: `true`
    pub fn allow_multiple(mut self, allow: bool) -> Self {
        self.allow_multiple = Some(allow);
        self
    }

    /// Set restore token from previous session
    ///
    /// Default: `None`
    pub fn restore_token(mut self, token: String) -> Self {
        self.restore_token = Some(token);
        self
    }

    /// Build the PortalConfig
    ///
    /// Uses defaults for any unspecified fields.
    pub fn build(self) -> PortalConfig {
        let defaults = PortalConfig::default();
        PortalConfig {
            cursor_mode: self.cursor_mode.unwrap_or(defaults.cursor_mode),
            persist_mode: self.persist_mode.unwrap_or(defaults.persist_mode),
            source_type: self.source_type.unwrap_or(defaults.source_type),
            devices: self.devices.unwrap_or(defaults.devices),
            allow_multiple: self.allow_multiple.unwrap_or(defaults.allow_multiple),
            restore_token: self.restore_token.or(defaults.restore_token),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PortalConfig::default();
        assert!(matches!(config.cursor_mode, CursorMode::Metadata));
        assert!(matches!(config.persist_mode, PersistMode::DoNot));
        assert!(config.allow_multiple);
        assert!(config.restore_token.is_none());
    }

    #[test]
    fn test_builder_with_defaults() {
        let config = PortalConfig::builder().build();
        assert!(matches!(config.cursor_mode, CursorMode::Metadata));
        assert!(matches!(config.persist_mode, PersistMode::DoNot));
    }

    #[test]
    fn test_builder_with_custom_values() {
        let config = PortalConfig::builder()
            .cursor_mode(CursorMode::Embedded)
            .persist_mode(PersistMode::Application)
            .allow_multiple(false)
            .restore_token("test-token".to_string())
            .build();

        assert!(matches!(config.cursor_mode, CursorMode::Embedded));
        assert!(matches!(config.persist_mode, PersistMode::Application));
        assert!(!config.allow_multiple);
        assert_eq!(config.restore_token, Some("test-token".to_string()));
    }

    #[test]
    fn test_struct_literal_with_defaults() {
        let config = PortalConfig {
            cursor_mode: CursorMode::Hidden,
            ..Default::default()
        };

        assert!(matches!(config.cursor_mode, CursorMode::Hidden));
        assert!(matches!(config.persist_mode, PersistMode::DoNot)); // Still default
    }
}
