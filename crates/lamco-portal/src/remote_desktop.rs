//! RemoteDesktop portal integration
//!
//! Provides input injection and screen capture via RemoteDesktop portal.

use std::os::fd::IntoRawFd;

use ashpd::desktop::remote_desktop::{
    DeviceType, KeyState, NotifyKeyboardKeycodeOptions, NotifyPointerAxisOptions, NotifyPointerButtonOptions,
    NotifyPointerMotionAbsoluteOptions, NotifyPointerMotionOptions, RemoteDesktop, SelectDevicesOptions, StartOptions,
};
use enumflags2::BitFlags;
use tracing::{debug, info};

use super::session::StreamInfo;
use crate::config::PortalConfig;
use crate::error::{PortalError, Result};

/// RemoteDesktop portal manager
///
/// Caches the ashpd RemoteDesktop proxy to avoid creating a new D-Bus proxy
/// on every input injection call.
pub struct RemoteDesktopManager {
    config: PortalConfig,
    proxy: RemoteDesktop,
}

impl RemoteDesktopManager {
    /// Create new RemoteDesktop manager
    pub async fn new(_connection: zbus::Connection, config: &PortalConfig) -> Result<Self> {
        info!("Initializing RemoteDesktop portal manager");
        let proxy = RemoteDesktop::new().await?;

        let version = proxy.version();
        info!("RemoteDesktop portal version: {}", version);

        Ok(Self {
            config: config.clone(),
            proxy,
        })
    }

    /// Get the portal interface version
    pub fn version(&self) -> u32 {
        self.proxy.version()
    }

    /// Create a remote desktop session
    pub async fn create_session(&self) -> Result<ashpd::desktop::Session<RemoteDesktop>> {
        info!("Creating RemoteDesktop session");

        let session = self.proxy.create_session(Default::default()).await?;

        debug!("RemoteDesktop session created");

        Ok(session)
    }

    /// Select devices for remote control
    pub async fn select_devices(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        devices: BitFlags<DeviceType>,
    ) -> Result<()> {
        info!("Selecting devices: {:?}", devices);

        let options = SelectDevicesOptions::default()
            .set_devices(devices)
            .set_persist_mode(self.config.persist_mode)
            .set_restore_token(self.config.restore_token.as_deref());

        self.proxy.select_devices(session, options).await?;

        info!("Devices selected successfully");
        Ok(())
    }

    /// Start the remote desktop session
    ///
    /// Returns: (PipeWire FD, Stream info, Optional restore token)
    ///
    /// The restore token, if present, should be stored and passed in future
    /// sessions via PortalConfig to avoid permission dialogs.
    pub async fn start_session(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
    ) -> Result<(std::os::fd::RawFd, Vec<StreamInfo>, Option<String>)> {
        info!("Starting RemoteDesktop session");

        let response = self.proxy.start(session, None, StartOptions::default()).await?;

        let selected = response.response()?;

        let restore_token = selected.restore_token().map(|s| s.to_string());

        if let Some(ref token) = restore_token {
            info!("Restore token received from portal (length: {} chars)", token.len());
            debug!("Restore token: {}", token);
        } else {
            debug!("No restore token in response (portal may not support persistence)");
        }

        // ashpd 0.13: streams() returns &[Stream] directly, no Option wrapper
        let streams = selected.streams();
        info!(
            "RemoteDesktop started with {} devices and {} streams",
            selected.devices().bits(),
            streams.len()
        );

        // Check if clipboard was actually granted (new in 0.13)
        if selected.is_clipboard_enabled() {
            info!("Clipboard access was granted by the portal");
        }

        // Get PipeWire FD
        use ashpd::desktop::screencast::Screencast;
        let screencast_proxy = Screencast::new().await?;
        let fd = screencast_proxy
            .open_pipe_wire_remote(session, Default::default())
            .await?;

        info!("PipeWire FD obtained: {:?}", fd);

        let stream_info: Vec<StreamInfo> = streams
            .iter()
            .map(|stream| {
                let node_id = stream.pipe_wire_node_id();
                let size = stream.size().unwrap_or((0, 0));
                let position = stream.position().unwrap_or((0, 0));
                let mapping_id = stream.mapping_id().map(str::to_owned);

                info!(
                    "Portal provided stream: node_id={}, size=({}, {}), position=({}, {}), mapping_id={:?}",
                    node_id, size.0, size.1, position.0, position.1, mapping_id
                );

                StreamInfo {
                    node_id,
                    position,
                    size: (
                        size.0.max(0).try_into().unwrap_or(0),
                        size.1.max(0).try_into().unwrap_or(0),
                    ),
                    source_type: super::session::SourceType::Monitor,
                    mapping_id,
                }
            })
            .collect();

        info!("Total streams from Portal: {}", stream_info.len());

        // Transfer FD ownership — PipeWire thread takes responsibility for closing it
        let raw_fd = fd.into_raw_fd();

        info!("FD {} ownership transferred to caller", raw_fd);

        Ok((raw_fd, stream_info, restore_token))
    }

    /// Inject pointer motion (relative)
    pub async fn notify_pointer_motion(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        self.proxy
            .notify_pointer_motion(session, dx, dy, NotifyPointerMotionOptions::default())
            .await?;
        Ok(())
    }

    /// Inject pointer motion (absolute in stream coordinates)
    pub async fn notify_pointer_motion_absolute(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        stream: u32,
        x: f64,
        y: f64,
    ) -> Result<()> {
        debug!("Injecting pointer motion: stream={}, x={:.2}, y={:.2}", stream, x, y);
        self.proxy
            .notify_pointer_motion_absolute(session, stream, x, y, NotifyPointerMotionAbsoluteOptions::default())
            .await
            .map_err(|e| PortalError::input_injection(format!("Pointer motion: {}", e)))?;
        debug!("Pointer motion injected successfully");
        Ok(())
    }

    /// Inject pointer button
    pub async fn notify_pointer_button(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        button: i32,
        pressed: bool,
    ) -> Result<()> {
        debug!("Injecting pointer button: button={}, pressed={}", button, pressed);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        self.proxy
            .notify_pointer_button(session, button, state, NotifyPointerButtonOptions::default())
            .await
            .map_err(|e| PortalError::input_injection(format!("Pointer button: {}", e)))?;
        debug!("Pointer button injected successfully");
        Ok(())
    }

    /// Inject pointer axis (scroll)
    pub async fn notify_pointer_axis(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        self.proxy
            .notify_pointer_axis(session, dx, dy, NotifyPointerAxisOptions::default().set_finish(true))
            .await?;
        Ok(())
    }

    /// Inject keyboard keysym (for Unicode input)
    ///
    /// Sends a keyboard event by XKB keysym rather than evdev keycode.
    /// Used for characters that don't map to a single keycode, such as
    /// CJK characters or accented letters. XKB Unicode keysyms use
    /// the range 0x01000000 + Unicode code point.
    pub async fn notify_keyboard_keysym(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        keysym: i32,
        pressed: bool,
    ) -> Result<()> {
        use ashpd::desktop::remote_desktop::NotifyKeyboardKeysymOptions;

        debug!(
            "Injecting keyboard keysym: keysym=0x{:08X}, pressed={}",
            keysym, pressed
        );
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        self.proxy
            .notify_keyboard_keysym(session, keysym, state, NotifyKeyboardKeysymOptions::default())
            .await
            .map_err(|e| PortalError::input_injection(format!("Keyboard keysym: {}", e)))?;
        debug!("Keyboard keysym event injected successfully");
        Ok(())
    }

    /// Inject keyboard key
    pub async fn notify_keyboard_keycode(
        &self,
        session: &ashpd::desktop::Session<RemoteDesktop>,
        keycode: i32,
        pressed: bool,
    ) -> Result<()> {
        debug!("Injecting keyboard: keycode={}, pressed={}", keycode, pressed);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        self.proxy
            .notify_keyboard_keycode(session, keycode, state, NotifyKeyboardKeycodeOptions::default())
            .await
            .map_err(|e| PortalError::input_injection(format!("Keyboard keycode: {}", e)))?;
        debug!("Keyboard event injected successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_remote_desktop_session_creation() {
        let connection = zbus::Connection::session().await.unwrap();
        let config = PortalConfig::default();

        let _manager = RemoteDesktopManager::new(connection, &config).await.unwrap();
    }
}
