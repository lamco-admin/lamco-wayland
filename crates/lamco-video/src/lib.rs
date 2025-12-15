//! # lamco-video
//!
//! Video frame processing and RDP bitmap conversion for Wayland screen capture.
//!
//! This crate is part of the [lamco-wayland](https://github.com/lamco-admin/lamco-wayland)
//! workspace and is designed to work with [`lamco-pipewire`](https://crates.io/crates/lamco-pipewire)
//! for video frame processing.
//!
//! # Features
//!
//! - **Frame Processing Pipeline**: Configurable video frame processing with rate limiting
//! - **RDP Bitmap Conversion**: Convert PipeWire frames to RDP-ready bitmap format
//! - **Damage Region Tracking**: Optimize updates by only sending changed regions
//! - **Buffer Pooling**: Efficient memory management with reusable buffers
//! - **Priority-Based Dispatch**: Multi-stream coordination with backpressure handling
//! - **SIMD Optimization**: Automatic use of SIMD instructions where available
//!
//! # Requirements
//!
//! This crate requires:
//! - **Linux** with a Wayland compositor
//! - **Rust 1.77+**
//! - **lamco-pipewire** for frame capture
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use lamco_video::{FrameProcessor, ProcessorConfig, BitmapConverter};
//! use lamco_pipewire::VideoFrame;
//! use tokio::sync::mpsc;
//!
//! // Create frame processor
//! let config = ProcessorConfig::default();
//! let processor = std::sync::Arc::new(FrameProcessor::new(config, 1920, 1080));
//!
//! // Create channels
//! let (input_tx, input_rx) = mpsc::channel(30);
//! let (output_tx, mut output_rx) = mpsc::channel(30);
//!
//! // Start processor
//! let processor_clone = processor.clone();
//! tokio::spawn(async move {
//!     processor_clone.start(input_rx, output_tx).await
//! });
//!
//! // Send frames from lamco-pipewire to processor
//! // Receive processed bitmap updates
//! while let Some(bitmap_update) = output_rx.recv().await {
//!     for rect in &bitmap_update.rectangles {
//!         println!("Update region: {:?}", rect.rectangle);
//!     }
//! }
//! ```
//!
//! # Architecture
//!
//! The processing pipeline:
//!
//! ```text
//! ┌────────────────────┐
//! │  lamco-pipewire    │
//! │  (VideoFrame)      │
//! └─────────┬──────────┘
//!           │
//!           ▼
//! ┌────────────────────┐
//! │  FrameDispatcher   │ ◄── Multi-stream routing
//! │  (priority queue)  │     Backpressure handling
//! └─────────┬──────────┘
//!           │
//!           ▼
//! ┌────────────────────┐
//! │  FrameProcessor    │ ◄── Frame rate limiting
//! │  (rate control)    │     Age-based dropping
//! └─────────┬──────────┘
//!           │
//!           ▼
//! ┌────────────────────┐
//! │  BitmapConverter   │ ◄── Pixel format conversion
//! │  (format conv)     │     Damage region tracking
//! └─────────┬──────────┘     Buffer pooling
//!           │
//!           ▼
//! ┌────────────────────┐
//! │  BitmapUpdate      │ ◄── RDP-ready rectangles
//! │  (RDP output)      │
//! └────────────────────┘
//! ```
//!
//! # Configuration
//!
//! ## Processor Configuration
//!
//! ```rust
//! use lamco_video::ProcessorConfig;
//!
//! let config = ProcessorConfig {
//!     target_fps: 60,           // Target frame rate
//!     max_queue_depth: 30,      // Max frames in queue before dropping
//!     adaptive_quality: true,   // Enable adaptive quality
//!     damage_threshold: 0.05,   // Minimum damage area to process (5%)
//!     drop_on_full_queue: true, // Drop frames when queue is full
//!     enable_metrics: true,     // Enable statistics collection
//! };
//! ```
//!
//! ## Dispatcher Configuration
//!
//! ```rust
//! use lamco_video::DispatcherConfig;
//!
//! let config = DispatcherConfig {
//!     channel_size: 30,          // Buffer size per stream
//!     priority_dispatch: true,   // Enable priority-based dispatch
//!     max_frame_age_ms: 150,     // Drop frames older than 150ms
//!     enable_backpressure: true, // Enable backpressure handling
//!     high_water_mark: 0.8,      // Trigger backpressure at 80%
//!     low_water_mark: 0.5,       // Release backpressure at 50%
//!     load_balancing: true,      // Enable load balancing
//! };
//! ```
//!
//! # RDP Pixel Formats
//!
//! The converter supports these RDP-compatible output formats:
//!
//! | Format | BPP | Description |
//! |--------|-----|-------------|
//! | BgrX32 | 4 | 32-bit BGRX (most common) |
//! | Bgr24  | 3 | 24-bit BGR |
//! | Rgb16  | 2 | 16-bit RGB 5:6:5 |
//! | Rgb15  | 2 | 15-bit RGB 5:5:5 |
//!
//! # Performance
//!
//! Typical performance on modern hardware:
//!
//! - **Conversion latency**: < 1ms per frame (1080p)
//! - **Memory usage**: < 50MB (with buffer pooling)
//! - **Throughput**: > 200 MB/s (with SIMD)
//! - **Frame rates**: Tested up to 144Hz
//!
//! # Cargo Features
//!
//! ```toml
//! [dependencies]
//! lamco-video = { version = "0.1", features = ["full"] }
//! ```
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `damage` | No | Full damage region tracking |
//! | `full` | No | All features enabled |

// =============================================================================
// CORE MODULES
// =============================================================================

pub mod converter;
pub mod dispatcher;
pub mod processor;

// =============================================================================
// RE-EXPORTS - PRIMARY API
// =============================================================================

// Converter types
pub use converter::{
    BitmapConverter, BitmapData, BitmapUpdate, ConversionError, ConversionStats, RdpPixelFormat,
    Rectangle,
};

// Dispatcher types
pub use dispatcher::{
    DispatchError, DispatcherConfig, DispatcherStats, FrameDispatcher, StreamPriority,
};

// Processor types
pub use processor::{FrameProcessor, ProcessingError, ProcessingStats, ProcessorConfig};

// =============================================================================
// CRATE-LEVEL ITEMS
// =============================================================================

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Recommended frame queue size for a given refresh rate
///
/// Returns the recommended channel buffer size to hold approximately
/// 500ms of frames, capped at 72 frames.
///
/// # Arguments
///
/// * `refresh_rate` - Monitor refresh rate in Hz
///
/// # Returns
///
/// Recommended channel buffer size (15-72)
#[must_use]
pub fn recommended_queue_size(refresh_rate: u32) -> usize {
    // Half second of frames, capped at 72
    ((refresh_rate / 2) as usize).clamp(15, 72)
}

/// Calculate RDP-compatible stride for a given width and format
///
/// RDP requires stride aligned to 64 bytes for optimal performance.
///
/// # Arguments
///
/// * `width` - Image width in pixels
/// * `format` - Target RDP pixel format
///
/// # Returns
///
/// Aligned stride in bytes
#[must_use]
pub fn calculate_rdp_stride(width: u32, format: RdpPixelFormat) -> u32 {
    let bpp = format.bytes_per_pixel() as u32;
    let row_bytes = width * bpp;
    // Align to 64 bytes
    (row_bytes + 63) & !63
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recommended_queue_size() {
        assert_eq!(recommended_queue_size(30), 15);
        assert_eq!(recommended_queue_size(60), 30);
        assert_eq!(recommended_queue_size(144), 72);
        assert_eq!(recommended_queue_size(240), 72); // Capped at 72
    }

    #[test]
    fn test_calculate_rdp_stride() {
        assert_eq!(calculate_rdp_stride(1920, RdpPixelFormat::BgrX32), 7680);
        assert_eq!(calculate_rdp_stride(1921, RdpPixelFormat::BgrX32), 7744);
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
