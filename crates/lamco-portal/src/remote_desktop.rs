//! RemoteDesktop portal integration
//!
//! Provides input injection and screen capture via RemoteDesktop portal.

use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
use enumflags2::BitFlags;
use std::os::fd::IntoRawFd;
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
    proxy: RemoteDesktop<'static>,
}

impl RemoteDesktopManager {
    /// Create new RemoteDesktop manager
    pub async fn new(_connection: zbus::Connection, config: &PortalConfig) -> Result<Self> {
        info!("Initializing RemoteDesktop portal manager");
        let proxy = RemoteDesktop::new().await?;
        Ok(Self {
            config: config.clone(),
            proxy,
        })
    }

    /// Create a remote desktop session
    pub async fn create_session(&self) -> Result<ashpd::desktop::Session<'static, RemoteDesktop<'static>>> {
        info!("Creating RemoteDesktop session");

        let session = self.proxy.create_session().await?;

        debug!("RemoteDesktop session created");

        Ok(session)
    }

    /// Select devices for remote control
    pub async fn select_devices(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        devices: BitFlags<DeviceType>,
    ) -> Result<()> {
        info!("Selecting devices: {:?}", devices);

        self.proxy
            .select_devices(
                session,
                devices,
                self.config.restore_token.as_deref(),
                self.config.persist_mode,
            )
            .await?;

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
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
    ) -> Result<(std::os::fd::RawFd, Vec<StreamInfo>, Option<String>)> {
        info!("Starting RemoteDesktop session");

        // Start returns a Request that resolves to SelectedDevices
        // None for headless/no parent window
        let response = self.proxy.start(session, None).await?;

        // Get the selected devices from the request response
        let selected = response.response()?;

        // Extract restore token from SelectedDevices (portal v4+)
        // The token allows restoring this session without user interaction
        let restore_token = selected.restore_token().map(|s| s.to_string());

        if let Some(ref token) = restore_token {
            info!("Restore token received from portal (length: {} chars)", token.len());
            debug!("Restore token: {}", token);
        } else {
            debug!("No restore token in response (portal may not support persistence)");
        }

        let stream_count = selected.streams().map(|s| s.len()).unwrap_or(0);
        info!(
            "RemoteDesktop started with {} devices and {} streams",
            selected.devices().bits(),
            stream_count
        );

        // Get PipeWire FD - note: open_pipe_wire_remote is on the Screencast trait/methods
        // For RemoteDesktop, we need to access streams differently
        // Actually, RemoteDesktop in 0.12.0 uses the screencast portal internally
        use ashpd::desktop::screencast::Screencast;
        let screencast_proxy = Screencast::new().await?;
        let fd = screencast_proxy.open_pipe_wire_remote(session).await?;

        info!("PipeWire FD obtained: {:?}", fd);

        // Convert stream info using new API
        let stream_info: Vec<StreamInfo> = selected
            .streams()
            .map(|streams| {
                streams
                    .iter()
                    .map(|stream| {
                        let node_id = stream.pipe_wire_node_id();
                        let size = stream.size().unwrap_or((0, 0));
                        let position = stream.position().unwrap_or((0, 0));

                        info!(
                            "Portal provided stream: node_id={}, size=({}, {}), position=({}, {})",
                            node_id, size.0, size.1, position.0, position.1
                        );

                        StreamInfo {
                            node_id,
                            position,
                            size: (
                                size.0.max(0).try_into().unwrap_or(0),
                                size.1.max(0).try_into().unwrap_or(0),
                            ),
                            source_type: super::session::SourceType::Monitor,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        info!("Total streams from Portal: {}", stream_info.len());

        // Transfer FD ownership — PipeWire thread takes responsibility for closing it
        let raw_fd = fd.into_raw_fd();

        info!("FD {} ownership transferred to caller", raw_fd);

        Ok((raw_fd, stream_info, restore_token))
    }

    /// Inject pointer motion (relative)
    pub async fn notify_pointer_motion(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        self.proxy.notify_pointer_motion(session, dx, dy).await?;
        Ok(())
    }

    /// Inject pointer motion (absolute in stream coordinates)
    pub async fn notify_pointer_motion_absolute(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        stream: u32,
        x: f64,
        y: f64,
    ) -> Result<()> {
        debug!("Injecting pointer motion: stream={}, x={:.2}, y={:.2}", stream, x, y);
        self.proxy
            .notify_pointer_motion_absolute(session, stream, x, y)
            .await
            .map_err(|e| PortalError::input_injection(format!("Pointer motion: {}", e)))?;
        debug!("Pointer motion injected successfully");
        Ok(())
    }

    /// Inject pointer button
    pub async fn notify_pointer_button(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        button: i32,
        pressed: bool,
    ) -> Result<()> {
        debug!("Injecting pointer button: button={}, pressed={}", button, pressed);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        self.proxy
            .notify_pointer_button(session, button, state)
            .await
            .map_err(|e| PortalError::input_injection(format!("Pointer button: {}", e)))?;
        debug!("Pointer button injected successfully");
        Ok(())
    }

    /// Inject pointer axis (scroll)
    pub async fn notify_pointer_axis(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        self.proxy.notify_pointer_axis(session, dx, dy, true).await?;
        Ok(())
    }

    /// Inject keyboard key
    pub async fn notify_keyboard_keycode(
        &self,
        session: &ashpd::desktop::Session<'_, RemoteDesktop<'_>>,
        keycode: i32,
        pressed: bool,
    ) -> Result<()> {
        debug!("Injecting keyboard: keycode={}, pressed={}", keycode, pressed);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        self.proxy
            .notify_keyboard_keycode(session, keycode, state)
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

        // This will trigger permission dialog
        // let session = manager.create_session().await;
        // assert!(session.is_ok());
    }
}
