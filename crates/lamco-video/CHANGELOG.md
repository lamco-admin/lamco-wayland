# Changelog

All notable changes to lamco-video will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2025-12-23

### Changed
- Updated dependency: lamco-pipewire 0.1.2 → 0.1.3

## [0.1.1] - 2025-12-17

### Fixed

- Added `#![cfg_attr(docsrs, feature(doc_cfg))]` for proper docs.rs conditional documentation
- Converted to workspace package inheritance (edition, rust-version, license, homepage, repository, authors)

### Added

- Added CHANGELOG.md

### Note

- docs.rs builds will fail for this crate because it depends on lamco-pipewire which requires `libpipewire-0.3` system library not available in the docs.rs build environment. This is expected and unavoidable.

## [0.1.0] - 2025-12-15

### Added

- Initial release on crates.io
- **`FrameProcessor`** - Video frame processing pipeline
  - Frame rate limiting with configurable target FPS
  - Age-based frame dropping
  - Queue depth management
  - Adaptive quality support
- **`ProcessorConfig`** - Processor configuration
  - Target FPS, queue depth, damage threshold
  - Metrics collection toggle
- **`BitmapConverter`** - RDP bitmap conversion
  - PipeWire frame to RDP bitmap format
  - Multiple output formats (BgrX32, Bgr24, Rgb16, Rgb15)
  - Buffer pooling for memory efficiency
- **`BitmapUpdate`** - RDP-ready output
  - Rectangle-based updates
  - Damage region optimization
- **`FrameDispatcher`** - Multi-stream coordination
  - Priority-based dispatch
  - Backpressure handling with high/low water marks
  - Load balancing across streams
- **`DispatcherConfig`** - Dispatcher configuration
  - Channel size, priority dispatch, max frame age
  - Backpressure thresholds
- Typed error handling with `ConversionError`, `ProcessingError`, `DispatchError`
- Statistics collection for monitoring

### Platform Support

- Linux only (requires lamco-pipewire)

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-video-v0.1.1...HEAD
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-video-v0.1.0...lamco-video-v0.1.1
[0.1.0]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-video-v0.1.0
