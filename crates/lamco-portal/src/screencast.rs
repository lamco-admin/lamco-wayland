//! ScreenCast portal integration
//!
//! Provides access to screen content via xdg-desktop-portal ScreenCast interface.

use ashpd::desktop::screencast::Screencast;
use std::os::fd::{AsRawFd, RawFd};
use tracing::{debug, info};

use super::session::StreamInfo;
use crate::config::PortalConfig;
use crate::error::Result;

/// ScreenCast portal manager
pub struct ScreenCastManager {
    #[allow(dead_code)]
    config: PortalConfig,
}

impl ScreenCastManager {
    /// Create new ScreenCast manager
    ///
    /// Note: The unused _connection parameter will be removed in a future version.
    /// ashpd creates its own connections internally.
    pub async fn new(_connection: zbus::Connection, config: &PortalConfig) -> Result<Self> {
        info!("Initializing ScreenCast portal manager");
        Ok(Self { config: config.clone() })
    }

    /// Create a screencast session
    pub async fn create_session(&self) -> Result<ashpd::desktop::Session<'static, Screencast<'static>>> {
        info!("Creating ScreenCast session");

        let proxy = Screencast::new().await?;
        let session = proxy.create_session().await?;

        debug!("ScreenCast session created");
        Ok(session)
    }

    /// Start the screencast and get PipeWire details
    pub async fn start(
        &self,
        session: &ashpd::desktop::Session<'_, Screencast<'_>>,
    ) -> Result<(RawFd, Vec<StreamInfo>)> {
        info!("Starting screencast session");

        let proxy = Screencast::new().await?;

        // Start returns a Request that resolves to Streams
        // None for headless/no parent window
        let streams_request = proxy.start(session, None).await?;

        // Get the streams from the request response
        let streams = streams_request.response()?;

        info!("Screencast started with {} streams", streams.streams().len());

        // Get PipeWire FD
        let fd = proxy.open_pipe_wire_remote(session).await?;

        let raw_fd = fd.as_raw_fd();
        info!("PipeWire FD obtained: {}", raw_fd);

        // Convert stream info using new API
        let stream_info: Vec<StreamInfo> = streams
            .streams()
            .iter()
            .map(|stream| {
                let size = stream.size().unwrap_or((0, 0));
                StreamInfo {
                    node_id: stream.pipe_wire_node_id(),
                    position: stream.position().unwrap_or((0, 0)),
                    size: (
                        size.0.max(0).try_into().unwrap_or(0),
                        size.1.max(0).try_into().unwrap_or(0),
                    ),
                    source_type: super::session::SourceType::Monitor, // Simplified for now
                }
            })
            .collect();

        // Don't close fd - we need to keep it
        std::mem::forget(fd);

        Ok((raw_fd, stream_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Portal tests require a running Wayland session with portal
    // These are integration tests that may not work in CI

    #[tokio::test]
    #[ignore] // Ignore in CI, run manually
    async fn test_screencast_manager_creation() {
        let connection = zbus::Connection::session().await.unwrap();
        let config = PortalConfig::default();

        let manager = ScreenCastManager::new(connection, &config).await;
        assert!(manager.is_ok());
    }
}
