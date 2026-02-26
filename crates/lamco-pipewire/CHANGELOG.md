# Changelog

All notable changes to lamco-pipewire will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.6] - 2026-02-26

### Changed

- **PipeWire 1.x MANDATORY flag for DmaBuf negotiation**
  - Format negotiation now produces two `EnumFormat` params when `use_dmabuf=true`:
    first with `MANDATORY | DONT_FIXATE` modifier property for DmaBuf, second without
    modifier for SHM fallback
  - PipeWire tries DmaBuf first and falls back to SHM if hardware can't satisfy it
  - Existing behavior preserved when `use_dmabuf=false` (SHM-only param)
- Enabled `pipewire/v0_3_33` and `libspa/v0_3_33` features for `PropertyFlags::DONT_FIXATE`

## [0.1.5] - 2026-02-26

### Changed

- Downgraded per-frame logging from `info!` to `trace!` in the process() callback
  - `mmap_fd_buffer()` entry/exit logging → `trace!`
  - `process() callback fired` → `trace!`
  - `Got buffer from stream` → `trace!`
  - Per-buffer type/size/offset/fd logging → `trace!`
  - MemPtr/MemFd copy logging → `trace!`
  - MemFd manual mmap logging → `trace!`
  - DMA-BUF first-time mmap and cache logging → `debug!`
  - Main loop heartbeat (every 1000 iterations) → `debug!`
  - One-time messages (stream created, format negotiated, state changes) remain at `info!`

### Added

- **Stream state push notifications** via `StreamStateEvent` channel
  - `PipeWireThreadManager::try_recv_state_event()` — non-blocking state poll
  - `PipeWireThreadManager::drain_state_events()` — drain all pending events
  - `StreamStateSnapshot` — Send-safe enum mirroring PipeWire stream states
  - Enables health monitoring without polling via `GetStreamState` commands
  - Events pushed from PipeWire thread's `state_changed` callback

## [0.1.4] - 2026-01-15

### Fixed

- Handle PipeWire size=0 "skip" frames gracefully
  - MemFd buffers with size=0 now logged and ignored instead of causing mmap failures
  - DmaBuf buffers with size=0 now logged and ignored instead of causing mmap failures
  - Eliminates "Invalid map size" errors during periods of no screen activity

### Changed

- Removed emojis from log messages for professional consistency

## [0.1.3] - 2025-12-23

### Changed
- Removed `stream.set_active()` call - let AUTOCONNECT flag handle activation
- Use `PW_ID_ANY` (None) instead of explicit node_id for portal streams

### Added
- Enhanced debug logging throughout stream lifecycle
- Periodic heartbeat logging (every 1000 iterations)
- Comprehensive stream state change logging

## [0.1.2] - 2025-12-17

### Fixed

- Added `#![cfg_attr(docsrs, feature(doc_cfg))]` for proper docs.rs conditional documentation
- Converted to workspace package inheritance (edition, rust-version, license, homepage, repository, authors)
- Fixed code formatting across the crate

### Added

- Added LICENSE-MIT and LICENSE-APACHE files to crate directory
- Added CHANGELOG.md

### Note

- docs.rs builds will fail for this crate because it requires `libpipewire-0.3` system library which is not available in the docs.rs build environment. This is expected and unavoidable.

## [0.1.1] - 2025-12-15

### Added

- Initial release on crates.io
- **`PipeWireManager`** - High-level Send + Sync wrapper for PipeWire
  - Stream creation and lifecycle management
  - Frame receiver channels for async frame access
  - Multi-stream support with coordinator
  - Automatic reconnection and error recovery
- **`PipeWireConfig`** - Configuration builder
  - Buffer count and format preferences
  - DMA-BUF enable/disable
  - Cursor and damage tracking options
  - Quality presets for different use cases
- **`VideoFrame`** - Captured frame with metadata
  - DMA-BUF and memory-mapped buffer support
  - Pixel format and stride information
  - Timestamp and damage regions
- **`MultiStreamCoordinator`** - Multi-monitor handling
  - Concurrent stream management
  - Frame synchronization
  - Monitor hotplug detection
- **`FrameDispatcher`** - Priority-based frame routing
  - Backpressure handling
  - Load balancing across streams
- **YUV conversion utilities** (with `yuv` feature)
  - NV12, I420, YUY2 to BGRA conversion
- **Hardware cursor extraction** (with `cursor` feature)
- **Damage region tracking** (with `damage` feature)
- **Adaptive bitrate control** (with `adaptive` feature)
- Typed error handling with `PipeWireError`
- Error classification for recovery decisions

### Architecture

- Dedicated PipeWire thread for non-Send types
- Command-based communication with async runtime
- Channel-based frame delivery

### Platform Support

- Linux only (Wayland required, PipeWire required)
- Tested on GNOME, KDE Plasma, Sway

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.6...HEAD
[0.1.6]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.5...lamco-pipewire-v0.1.6
[0.1.5]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.4...lamco-pipewire-v0.1.5
[0.1.4]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.3...lamco-pipewire-v0.1.4
[0.1.3]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.2...lamco-pipewire-v0.1.3
[0.1.2]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.1...lamco-pipewire-v0.1.2
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-pipewire-v0.1.1
