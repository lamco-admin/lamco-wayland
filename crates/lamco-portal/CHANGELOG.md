# Changelog

All notable changes to lamco-portal will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2025-12-17

### Fixed

- Added `#![cfg_attr(docsrs, feature(doc_cfg))]` for proper docs.rs conditional documentation
- Converted to workspace package inheritance (edition, rust-version, license, homepage, repository, authors)
- Converted to workspace lint inheritance

### Added

- Added LICENSE-MIT and LICENSE-APACHE files to crate directory
- Added CHANGELOG.md

## [0.1.1] - 2025-12-15

### Added

- Initial release on crates.io
- **`PortalManager`** - Main entry point for portal interactions
  - Session creation with ScreenCast + RemoteDesktop combined sessions
  - Clipboard integration support
  - Graceful resource cleanup
- **`PortalConfig`** - Configuration builder
  - Cursor mode selection (hidden, embedded, metadata)
  - Source type selection (monitors, windows)
  - Persist mode for remembering permissions
  - Multi-monitor support
- **`PortalSessionHandle`** - Session state management
  - PipeWire file descriptor access
  - Stream information (node ID, position, size)
  - ashpd session reference for input injection
- **`ScreenCastManager`** - Screen capture coordination
- **`RemoteDesktopManager`** - Input injection (keyboard, mouse, scroll)
- **`ClipboardManager`** - Portal-based clipboard access
- **`PortalClipboardSink`** - Integration with lamco-clipboard-core (optional)
- **`DbusClipboardBridge`** - D-Bus clipboard for GNOME fallback (optional)
- Typed error handling with `PortalError`
- Re-exports of ashpd types for convenience

### Platform Support

- Linux only (Wayland required)
- Tested on GNOME, KDE Plasma, Sway

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-portal-v0.1.2...HEAD
[0.1.2]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-portal-v0.1.1...lamco-portal-v0.1.2
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-portal-v0.1.1
