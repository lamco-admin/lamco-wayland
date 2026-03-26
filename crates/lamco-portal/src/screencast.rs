//! ScreenCast portal integration
//!
//! Provides access to screen content via xdg-desktop-portal ScreenCast interface.

use std::os::fd::{IntoRawFd, RawFd};

use ashpd::desktop::screencast::Screencast;
use tracing::{debug, info};

use super::session::StreamInfo;
use crate::config::PortalConfig;
use crate::error::Result;

/// ScreenCast portal manager
///
/// Caches the ashpd Screencast proxy to avoid creating a new D-Bus proxy
/// on every operation.
pub struct ScreenCastManager {
    #[expect(dead_code, reason = "config reserved for future use")]
    config: PortalConfig,
    proxy: Screencast,
}

impl ScreenCastManager {
    /// Create new ScreenCast manager
    pub async fn new(_connection: zbus::Connection, config: &PortalConfig) -> Result<Self> {
        info!("Initializing ScreenCast portal manager");
        let proxy = Screencast::new().await?;
        Ok(Self {
            config: config.clone(),
            proxy,
        })
    }

    /// Create a screencast session
    pub async fn create_session(&self) -> Result<ashpd::desktop::Session<Screencast>> {
        info!("Creating ScreenCast session");

        let session = self.proxy.create_session(Default::default()).await?;

        debug!("ScreenCast session created");
        Ok(session)
    }

    /// Start the screencast and get PipeWire details
    pub async fn start(&self, session: &ashpd::desktop::Session<Screencast>) -> Result<(RawFd, Vec<StreamInfo>)> {
        info!("Starting screencast session");

        let streams_request = self.proxy.start(session, None, Default::default()).await?;

        let streams = streams_request.response()?;

        info!("Screencast started with {} streams", streams.streams().len());

        // Get PipeWire FD
        let fd = self.proxy.open_pipe_wire_remote(session, Default::default()).await?;

        // Transfer FD ownership — caller takes responsibility for closing it
        let raw_fd = fd.into_raw_fd();
        info!("PipeWire FD obtained: {}", raw_fd);

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
                    source_type: super::session::SourceType::Monitor,
                }
            })
            .collect();

        Ok((raw_fd, stream_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Ignore in CI, run manually
    async fn test_screencast_manager_creation() {
        let connection = zbus::Connection::session().await.unwrap();
        let config = PortalConfig::default();

        let manager = ScreenCastManager::new(connection, &config).await;
        assert!(manager.is_ok());
    }
}
