# lamco-pipewire

High-performance PipeWire integration for Wayland screen capture with DMA-BUF support.

[![Crates.io](https://img.shields.io/crates/v/lamco-pipewire.svg)](https://crates.io/crates/lamco-pipewire)
[![Documentation](https://docs.rs/lamco-pipewire/badge.svg)](https://docs.rs/lamco-pipewire)
[![License](https://img.shields.io/crates/l/lamco-pipewire.svg)](LICENSE-MIT)

**[Website](https://lamco.ai/open-source/lamco-wayland/pipewire/)** · **[Documentation](https://docs.rs/lamco-pipewire)** · **[Source](https://github.com/lamco-admin/lamco-wayland)**

## Features

- **Zero-Copy DMA-BUF**: Hardware-accelerated frame transfer when available
- **Multi-Monitor**: Concurrent handling of multiple monitor streams
- **Format Negotiation**: Automatic format selection with fallbacks
- **YUV Conversion**: Built-in NV12, I420, YUY2 to BGRA conversion
- **Cursor Extraction**: Separate cursor tracking — position and, since 0.6.0, the real cursor image (`CursorMeta::bitmap`)
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
lamco-pipewire = { version = "0.6", features = ["full"] }
```

## Versions & compatibility

`lamco-pipewire` ships **two parallel supported lines on the PipeWire/SPA 0.10
bindings**, plus a legacy 0.9-era line. They differ mainly in their system
**libpipewire floor** and their metadata internals:

| Line | Latest | PipeWire/SPA bindings | Metadata internals | libpipewire floor | Cursor bitmap |
|------|--------|-----------------------|--------------------|-------------------|---------------|
| **0.6.x** (modern head) | **0.6.2** | 0.10 | safe `find_meta` wrappers (`unsafe`-free) | **0.3.62** | ✅ |
| **0.5.x** (low floor) | **0.5.1** | 0.10 | raw `libspa_sys` FFI | **0.3.33** | — |
| 0.4.x (legacy) | 0.4.5 | 0.9 | raw `libspa_sys` FFI | 0.3.33 | — |

- **New code → `0.6` (0.6.2):** safe metadata internals, real cursor pixels,
  current deps; needs system **libpipewire ≥ 0.3.62** (present on every
  currently-supported distro).
- **Older/minimal environments → `0.5` (0.5.1):** same 0.10 bindings and the
  **same DMA-BUF race fix**, with a lower floor (**libpipewire ≥ 0.3.33**).

Both 0.5.1 and 0.6.2 contain the DMA-BUF mmap-cache cross-thread race fix; on
0.5.0 / 0.6.0 / 0.6.1, update within your line. The two lines are not
semver-compatible with each other — pin to one deliberately. Full detail:
[`docs/COMPATIBILITY.md`](https://github.com/lamco-admin/lamco-wayland/blob/master/docs/COMPATIBILITY.md).

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
- **PipeWire development libraries**: `libpipewire-0.3-dev` (Debian/Ubuntu) or `pipewire-devel` (Fedora) — **≥ 0.3.62** for the 0.6.x line, **≥ 0.3.33** for the 0.5.x line
- **Rust 1.87+** (edition 2024)

## Platform Compatibility

| Compositor | Portal Package | Status |
|------------|----------------|--------|
| GNOME | `xdg-desktop-portal-gnome` | ✅ Tested |
| KDE Plasma | `xdg-desktop-portal-kde` | ✅ Tested |
| wlroots (Sway, Hyprland) | `xdg-desktop-portal-wlr` | ✅ Tested |
| X11 | N/A | ❌ Not supported |

## Related Crates

- [`lamco-portal`](https://crates.io/crates/lamco-portal) - XDG Desktop Portal integration for obtaining PipeWire file descriptors

## About

Developed by [Lamco Development LLC](https://lamco.ai/open-source/lamco-wayland/pipewire/). Part of the lamco-wayland ecosystem for building Wayland-native applications in Rust.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
