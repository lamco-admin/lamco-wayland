# Changelog

All notable changes to the lamco-wayland workspace will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.6] - 2026-02-26

### Changed
- Updated lamco-pipewire dependency to v0.2.0 (OwnedFd API, MANDATORY DmaBuf, stream state push)
- Updated lamco-video dependency to v0.1.3 (lamco-pipewire 0.2.0 compatibility)

## [0.2.5] - 2026-01-29

### Changed
- Updated lamco-portal dependency to v0.3.1 (clipboard timing and session cleanup fixes)

## [0.2.4] - 2026-01-15

### Changed
- Updated lamco-pipewire dependency to v0.1.4 (size=0 frame handling fix)

## [0.2.3] - 2025-12-31

### Changed
- Updated lamco-portal dependency to v0.3.0 (restore token support)

## [0.2.0] - 2025-12-21

### Changed
- Updated lamco-portal to v0.2.0 (adds `dbus-clipboard` feature)

## [0.1.1] - 2025-12-17

### Fixed

- Updated dependencies to latest versions:
  - lamco-portal 0.1.2
  - lamco-pipewire 0.1.2
  - lamco-video 0.1.1

### Added

- Added CHANGELOG.md

### Note

- docs.rs builds will fail for this crate because it depends on lamco-pipewire which requires `libpipewire-0.3` system library not available in the docs.rs build environment. This is expected and unavoidable.

## [0.1.0] - 2025-12-15

### Added

- Initial release on crates.io
- **`lamco-wayland`** meta-crate providing unified access to:
  - `lamco-portal` - XDG Desktop Portal integration
  - `lamco-pipewire` - PipeWire screen capture
  - `lamco-video` - Video frame processing
- Feature flags for selective inclusion:
  - `portal` (default) - XDG Portal integration
  - `pipewire` (default) - PipeWire capture
  - `video` (default) - Video processing
  - `full` - All features from all sub-crates
- Prelude module with commonly used types
- Comprehensive documentation with architecture diagrams

### Platform Support

- Linux only (Wayland required)
- Tested on GNOME, KDE Plasma, Sway

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-wayland-v0.2.4...HEAD
[0.2.4]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-wayland-v0.2.3...lamco-wayland-v0.2.4
[0.2.3]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-wayland-v0.2.0...lamco-wayland-v0.2.3
[0.2.0]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-wayland-v0.1.1...lamco-wayland-v0.2.0
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-wayland-v0.1.0...lamco-wayland-v0.1.1
[0.1.0]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-wayland-v0.1.0
