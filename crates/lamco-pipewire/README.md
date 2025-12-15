# lamco-pipewire

High-performance PipeWire integration for Wayland screen capture with DMA-BUF support.

[![Crates.io](https://img.shields.io/crates/v/lamco-pipewire.svg)](https://crates.io/crates/lamco-pipewire)
[![Documentation](https://docs.rs/lamco-pipewire/badge.svg)](https://docs.rs/lamco-pipewire)
[![License](https://img.shields.io/crates/l/lamco-pipewire.svg)](LICENSE-MIT)

## Features

- **Zero-Copy DMA-BUF**: Hardware-accelerated frame transfer when available
- **Multi-Monitor**: Concurrent handling of multiple monitor streams
- **Format Negotiation**: Automatic format selection with fallbacks
- **YUV Conversion**: Built-in NV12, I420, YUY2 to BGRA conversion
- **Cursor Extraction**: Separate cursor tracking for remote desktop
- **Damage Tracking**: Region-based change detection for efficient encoding
- **Adaptive Bitrate**: Network-aware bitrate control for streaming
- **Error Recovery**: Automatic reconnection and stream recovery

## Quick Start

```rust,ignore
use lamco_pipewire::{PipeWireManager, PipeWireConfig, StreamInfo, SourceType};

// Create manager with default configuration
let mut manager = PipeWireManager::with_default()?;

// Connect using portal-provided file descriptor (from lamco-portal)
manager.connect(fd).await?;

// Create stream for a monitor
let stream_info = StreamInfo {
    node_id: 42,
    position: (0, 0),
    size: (1920, 1080),
    source_type: SourceType::Monitor,
};

let handle = manager.create_stream(&stream_info).await?;

// Receive frames
if let Some(mut rx) = manager.frame_receiver(handle.id).await {
    while let Some(frame) = rx.recv().await {
        println!("Frame: {}x{}", frame.width, frame.height);
    }
}

manager.shutdown().await?;
```

## Configuration

```rust
use lamco_pipewire::{PipeWireConfig, PixelFormat};

let config = PipeWireConfig::builder()
    .buffer_count(4)                      // More buffers for high refresh
    .preferred_format(PixelFormat::BGRA)  // Preferred pixel format
    .use_dmabuf(true)                     // Enable zero-copy
    .max_streams(4)                       // Limit concurrent streams
    .enable_cursor(true)                  // Extract cursor separately
    .enable_damage_tracking(true)         // Track changed regions
    .build();

let manager = PipeWireManager::new(config)?;
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `dmabuf` | Yes | DMA-BUF zero-copy support |
| `yuv` | No | YUV format conversion utilities |
| `cursor` | No | Hardware cursor extraction |
| `damage` | No | Region damage tracking |
| `adaptive` | No | Adaptive bitrate control |
| `full` | No | All features enabled |

```toml
[dependencies]
lamco-pipewire = { version = "0.1", features = ["full"] }
```

## Architecture

PipeWire's Rust bindings use `Rc<>` and `NonNull<>` internally, making them **not Send**. This crate solves this with a dedicated thread architecture:

```text
┌─────────────────────────────────────────────────────────┐
│              Tokio Async Runtime                        │
│                                                         │
│  Your Application → PipeWireManager                     │
│                    (Send + Sync wrapper)                │
│                           │                             │
│                           │ Commands via mpsc           │
│                           ▼                             │
└───────────────────────────┼─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│         Dedicated PipeWire Thread                       │
│         (std::thread - owns all non-Send types)         │
│                                                         │
│  MainLoop (Rc) ─> Context (Rc) ─> Core (Rc)            │
│                                      │                  │
│                                      ▼                  │
│                              Streams (NonNull)          │
│                                      │                  │
│                                      │ Frames via mpsc  │
└──────────────────────────────────────┼──────────────────┘
                                       │
                                       ▼
                             Your application receives frames
```

## Performance

- **Frame latency**: < 2ms (with DMA-BUF)
- **Memory usage**: < 100MB per stream
- **CPU usage**: < 5% per stream (1080p @ 60Hz)
- **Refresh rates**: Tested up to 144Hz

## Requirements

- **Linux** with a Wayland compositor
- **PipeWire** installed and running
- **PipeWire development libraries**: `libpipewire-0.3-dev` (Debian/Ubuntu) or `pipewire-devel` (Fedora)
- **Rust 1.77+**

## Platform Compatibility

| Compositor | Portal Package | Status |
|------------|----------------|--------|
| GNOME | `xdg-desktop-portal-gnome` | ✅ Tested |
| KDE Plasma | `xdg-desktop-portal-kde` | ✅ Tested |
| wlroots (Sway, Hyprland) | `xdg-desktop-portal-wlr` | ✅ Tested |
| X11 | N/A | ❌ Not supported |

## Related Crates

- [`lamco-portal`](https://crates.io/crates/lamco-portal) - XDG Desktop Portal integration for obtaining PipeWire file descriptors

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
