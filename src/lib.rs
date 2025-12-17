//! # lamco-wayland
//!
//! Wayland screen capture, XDG Portal integration, and video processing for Rust.
//!
//! This crate provides a unified interface to the lamco Wayland libraries:
//!
//! - **[`portal`]** - XDG Desktop Portal integration (screencast, remote desktop, clipboard)
//! - **[`pipewire`]** - PipeWire screen capture with DMA-BUF support
//! - **[`video`]** - Video frame processing and RDP bitmap conversion
//!
//! # Features
//!
//! All features are enabled by default. You can selectively enable only what you need:
//!
//! ```toml
//! # Use everything (default)
//! lamco-wayland = "0.1"
//!
//! # Portal only
//! lamco-wayland = { version = "0.1", default-features = false, features = ["portal"] }
//!
//! # Portal + PipeWire
//! lamco-wayland = { version = "0.1", default-features = false, features = ["portal", "pipewire"] }
//!
//! # All features including sub-crate features
//! lamco-wayland = { version = "0.1", features = ["full"] }
//! ```
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `portal` | Yes | XDG Desktop Portal integration |
//! | `pipewire` | Yes | PipeWire screen capture |
//! | `video` | Yes | Video frame processing |
//! | `full` | No | All features from all sub-crates |
//!
//! # Quick Start
//!
//! ## Screen Capture with Portal
//!
//! ```rust,ignore
//! use lamco_wayland::portal::{PortalManager, PortalConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create portal manager and start session
//!     let manager = PortalManager::with_default().await?;
//!     let session = manager.create_session("my-app".to_string(), None).await?;
//!
//!     // Get PipeWire connection info
//!     let fd = session.pipewire_fd();
//!     let streams = session.streams();
//!
//!     println!("Capturing {} streams", streams.len());
//!     Ok(())
//! }
//! ```
//!
//! ## Full Pipeline: Portal → PipeWire → Video
//!
//! ```rust,ignore
//! use lamco_wayland::{
//!     portal::PortalManager,
//!     pipewire::{PipeWireManager, PipeWireConfig},
//!     video::{FrameProcessor, ProcessorConfig, BitmapConverter},
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 1. Request screen capture via portal
//!     let portal = PortalManager::with_default().await?;
//!     let session = portal.create_session("capture".to_string(), None).await?;
//!
//!     // 2. Connect to PipeWire
//!     let config = PipeWireConfig::builder()
//!         .fd(session.pipewire_fd())
//!         .node_id(session.streams()[0].node_id)
//!         .build();
//!     let pw = PipeWireManager::new(config)?;
//!
//!     // 3. Process frames
//!     let processor = FrameProcessor::new(ProcessorConfig::default(), 1920, 1080);
//!
//!     // ... receive frames and convert to RDP bitmaps
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        lamco-wayland                            │
//! ├─────────────────┬─────────────────────┬─────────────────────────┤
//! │   lamco-portal  │   lamco-pipewire    │      lamco-video        │
//! │                 │                     │                         │
//! │  PortalManager  │  PipeWireManager    │  BitmapConverter        │
//! │  SessionHandle  │  VideoFrame         │  FrameProcessor         │
//! │  PortalConfig   │  PipeWireConfig     │  FrameDispatcher        │
//! └────────┬────────┴──────────┬──────────┴────────────┬────────────┘
//!          │                   │                       │
//!          ▼                   ▼                       ▼
//!    XDG Desktop Portal   PipeWire API            RDP Bitmap Format
//! ```
//!
//! # Platform Support
//!
//! - **Linux only** - Requires Wayland compositor
//! - **PipeWire** - Required for lamco-pipewire
//! - **XDG Desktop Portal** - Required for lamco-portal
//!
//! Tested on GNOME, KDE Plasma, and Sway.
//!
//! # Related Crates
//!
//! You can also use the individual crates directly:
//!
//! - [`lamco-portal`](https://crates.io/crates/lamco-portal) - Portal only
//! - [`lamco-pipewire`](https://crates.io/crates/lamco-pipewire) - PipeWire only
//! - [`lamco-video`](https://crates.io/crates/lamco-video) - Video processing only

#![cfg_attr(docsrs, feature(doc_cfg))]

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// =============================================================================
// RE-EXPORTS
// =============================================================================

/// XDG Desktop Portal integration for Wayland screen capture and input control.
///
/// This module provides access to the XDG Desktop Portal APIs for:
/// - Screen casting (capturing screen content)
/// - Remote desktop (keyboard/mouse input)
/// - Clipboard access
///
/// See [`lamco_portal`] documentation for details.
#[cfg(feature = "portal")]
#[cfg_attr(docsrs, doc(cfg(feature = "portal")))]
pub use lamco_portal as portal;

/// High-performance PipeWire screen capture with DMA-BUF support.
///
/// This module provides access to PipeWire for video capture:
/// - Zero-copy DMA-BUF buffer sharing
/// - Multiple format support (BGRA, RGBA, NV12, etc.)
/// - Damage region tracking
/// - Hardware cursor extraction
///
/// See [`lamco_pipewire`] documentation for details.
#[cfg(feature = "pipewire")]
#[cfg_attr(docsrs, doc(cfg(feature = "pipewire")))]
pub use lamco_pipewire as pipewire;

/// Video frame processing and RDP bitmap conversion.
///
/// This module provides video processing utilities:
/// - Frame rate limiting and queueing
/// - Pixel format conversion
/// - RDP bitmap generation
/// - Damage region optimization
///
/// See [`lamco_video`] documentation for details.
#[cfg(feature = "video")]
#[cfg_attr(docsrs, doc(cfg(feature = "video")))]
pub use lamco_video as video;

// =============================================================================
// PRELUDE - Common types for convenience
// =============================================================================

/// Prelude module with commonly used types.
///
/// ```rust
/// use lamco_wayland::prelude::*;
/// ```
pub mod prelude {
    #[cfg(feature = "portal")]
    pub use lamco_portal::{PortalConfig, PortalError, PortalManager, PortalSessionHandle};

    #[cfg(feature = "pipewire")]
    pub use lamco_pipewire::{PipeWireConfig, PipeWireError, PipeWireManager, VideoFrame};

    #[cfg(feature = "video")]
    pub use lamco_video::{BitmapConverter, BitmapUpdate, FrameDispatcher, FrameProcessor, ProcessorConfig};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    #[cfg(feature = "portal")]
    fn test_portal_reexport() {
        // Just verify the re-export works
        let _ = portal::PortalConfig::default();
    }

    #[test]
    #[cfg(feature = "video")]
    fn test_video_reexport() {
        // Just verify the re-export works
        let _ = video::ProcessorConfig::default();
    }
}
