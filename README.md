# Lamco Wayland Libraries

Rust libraries for Wayland screen capture, XDG Portal integration, and video processing.

[![Crates.io](https://img.shields.io/crates/v/lamco-wayland.svg)](https://crates.io/crates/lamco-wayland)
[![Documentation](https://docs.rs/lamco-wayland/badge.svg)](https://docs.rs/lamco-wayland)
[![CI](https://github.com/lamco-admin/lamco-wayland/actions/workflows/ci.yml/badge.svg)](https://github.com/lamco-admin/lamco-wayland/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](README.md#license)

**[Website](https://lamco.ai/open-source/lamco-wayland/)** · **[Documentation](https://docs.rs/lamco-wayland)** · **[Source](https://github.com/lamco-admin/lamco-wayland)**

## Crates

| Crate | Version | Description |
|-------|---------|-------------|
| [lamco-wayland](https://crates.io/crates/lamco-wayland) | [![Crates.io](https://img.shields.io/crates/v/lamco-wayland.svg)](https://crates.io/crates/lamco-wayland) | Meta-crate with all libraries |
| [lamco-portal](https://crates.io/crates/lamco-portal) | [![Crates.io](https://img.shields.io/crates/v/lamco-portal.svg)](https://crates.io/crates/lamco-portal) | XDG Desktop Portal integration |
| [lamco-pipewire](https://crates.io/crates/lamco-pipewire) | [![Crates.io](https://img.shields.io/crates/v/lamco-pipewire.svg)](https://crates.io/crates/lamco-pipewire) | PipeWire screen capture |
| [lamco-video](https://crates.io/crates/lamco-video) | [![Crates.io](https://img.shields.io/crates/v/lamco-video.svg)](https://crates.io/crates/lamco-video) | Video processing & RDP bitmap conversion |

## Quick Start

```toml
[dependencies]
# Use everything
lamco-wayland = "0.5"

# Or select what you need
lamco-wayland = { version = "0.5", default-features = false, features = ["portal"] }
```

```rust
use lamco_wayland::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create portal manager
    let manager = PortalManager::with_default().await?;

    // Create session (triggers permission dialog)
    let session = manager.create_session("my-session".to_string(), None).await?;

    // Access PipeWire for video capture
    let fd = session.pipewire_fd();
    let streams = session.streams();

    println!("Capturing {} streams on PipeWire FD {}", streams.len(), fd);

    Ok(())
}
```

## Versions & compatibility

The capture crates ship **two parallel supported lines on the PipeWire/SPA 0.10
bindings**, plus a legacy 0.9-era line. The meta-crate version tracks the line:

| Line | `lamco-wayland` | `lamco-pipewire` | `lamco-portal` | `lamco-video` | PipeWire bindings | libpipewire floor |
|------|-----------------|------------------|----------------|---------------|-------------------|-------------------|
| **0.6.x** (modern head) | **0.6.14** | 0.6.12 | 0.4.5 | 0.3.0 | 0.10 | **0.3.62** |
| **0.5.x** (low floor) | **0.5.13** | 0.5.12 | 0.4.4 | 0.2.0 | 0.10 | **0.3.33** |
| 0.4.x (legacy) | 0.4.7 | 0.4.5 | 0.4.1 | 0.1.10 | 0.9 | 0.3.33 |

- **New code → `lamco-wayland = "0.6"`** — safe (`unsafe`-free) PipeWire metadata
  internals, real cursor pixels, current dependencies; needs system
  **libpipewire ≥ 0.3.62**.
- **Older/minimal environments → `lamco-wayland = "0.5"`** — same 0.10 bindings
  and the same DMA-BUF race fix, with a lower floor (**libpipewire ≥ 0.3.33**).
  This is the 0.5.x maintenance line.

Both current lines carry the same DMA-BUF mmap-cache fixes, including the
buffer-removal eviction added in `lamco-pipewire` 0.5.12 / 0.6.12, and the
DestroyStream release-ordering fix. MSRV is **Rust 1.87** (edition 2024) on every
current line. The 0.5 and 0.6 lines are not semver-compatible with each other —
pin to one deliberately. Full detail:
[`docs/COMPATIBILITY.md`](https://github.com/lamco-admin/lamco-wayland/blob/master/docs/COMPATIBILITY.md).

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `portal` | Yes | XDG Desktop Portal integration |
| `pipewire` | Yes | PipeWire screen capture |
| `video` | Yes | Video frame processing |
| `full` | No | All features from all sub-crates |

## Use Cases

- **RDP servers** - Lamco RDP Server, custom implementations
- **VNC servers** - Wayland support for VNC
- **Screen recording tools** - Capture Wayland displays
- **Video conferencing** - Screen sharing applications
- **Computer vision** - Process Wayland screen content
- **Accessibility tools** - Screen readers, automation

## Requirements

- **Wayland compositor** - GNOME, KDE Plasma, Sway, etc.
- **xdg-desktop-portal** - Desktop Portal implementation
- **PipeWire** - For video streaming (lamco-pipewire only)

Not compatible with X11 - Wayland only.

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                        lamco-wayland                            │
├─────────────────┬─────────────────────┬─────────────────────────┤
│   lamco-portal  │   lamco-pipewire    │      lamco-video        │
│                 │                     │                         │
│  PortalManager  │  PipeWireManager    │  BitmapConverter        │
│  SessionHandle  │  VideoFrame         │  FrameProcessor         │
│  PortalConfig   │  PipeWireConfig     │  FrameDispatcher        │
└────────┬────────┴──────────┬──────────┴────────────┬────────────┘
         │                   │                       │
         ▼                   ▼                       ▼
   XDG Desktop Portal   PipeWire API            RDP Bitmap Format
```

## Platform Support

| Compositor | Status | Backend |
|------------|--------|---------|
| GNOME | ✅ Tested | xdg-desktop-portal-gnome |
| KDE Plasma | ✅ Tested | xdg-desktop-portal-kde |
| Sway / wlroots | ✅ Tested | xdg-desktop-portal-wlr |
| Hyprland | ⚠️ Should work | xdg-desktop-portal-hyprland |
| Other Wayland | ⚠️ May work | Depends on portal backend |
| X11 | ❌ Not supported | Wayland only |

## Development

```bash
# Clone repository
git clone https://github.com/lamco-admin/lamco-wayland.git
cd lamco-wayland

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Build documentation
cargo doc --no-deps --workspace --open
```

## About

These libraries are extracted from the [Lamco RDP Server](https://lamco.ai/products/lamco-rdp-server/) project but designed for general use. They work with any Wayland compositor and are not RDP-specific.

Built with production-tested code from real-world remote desktop deployment.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Links

- [Documentation](https://docs.rs/lamco-wayland)
- [Crates.io](https://crates.io/crates/lamco-wayland)
- [GitHub](https://github.com/lamco-admin/lamco-wayland)
- [Lamco RDP Server](https://lamco.ai/products/lamco-rdp-server/)
