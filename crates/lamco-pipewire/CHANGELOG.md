# Changelog

All notable changes to lamco-pipewire will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.2...HEAD
[0.1.2]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.1...lamco-pipewire-v0.1.2
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-pipewire-v0.1.1
