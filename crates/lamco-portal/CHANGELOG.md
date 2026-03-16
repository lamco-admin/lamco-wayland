# Changelog

All notable changes to lamco-portal will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.3] - 2026-03-15

### Changed
- Bump to Rust edition 2024, minimum supported Rust version 1.85

## [0.3.2] - 2026-03-04

### Changed
- Upgrade to zbus 5 and ashpd 0.12.3
- Code cleanup and reduced boilerplate

## [0.3.1] - 2026-01-29

### Fixed
- Fixed clipboard request timing - call immediately after CreateSession instead of after SelectDevices/SelectSources
  - Portal spec requires clipboard.request() when session state is INIT
  - Resolves "Invalid state" errors from portal daemon on GNOME Flatpak
- Added session cleanup in all error paths to prevent orphaned D-Bus session objects
  - Portal sessions now properly closed when device/source selection fails
  - Prevents stale session state that causes subsequent clipboard requests to fail

### Changed
- Improved code quality and comment clarity
- Reduced verbose logging in clipboard operations

## [0.3.0] - 2025-12-31

### Added
- **Restore token support** for session persistence (Portal v4+ required)
  - `create_session()` now returns `(PortalSessionHandle, Option<String>)` with restore token
  - `start_session()` now returns restore token from portal response
  - Enables unattended operation - no permission dialogs on subsequent runs
  - Token should be stored securely and passed via `PortalConfig::restore_token`

### Changed
- **BREAKING:** `PortalManager::create_session()` return type changed from `Result<PortalSessionHandle>` to `Result<(PortalSessionHandle, Option<String>)>`
- **BREAKING:** `RemoteDesktopManager::start_session()` return type changed from `Result<(RawFd, Vec<StreamInfo>)>` to `Result<(RawFd, Vec<StreamInfo>, Option<String>)>`
- Default `persist_mode` changed from `DoNot` to `ExplicitlyRevoked` for better session persistence UX

### Notes
- Restore tokens are only available on Portal v4+ (GNOME 45+, KDE Plasma 6+)
- Tokens should be stored securely (e.g., system keyring, encrypted file)
- If portal doesn't support tokens, returns `None` (behavior unchanged for Portal v3)
- See examples and documentation for proper token storage and restoration patterns

## [0.2.2] - 2025-12-24

### Fixed
- Fixed non-blocking FD handling for clipboard read (EAGAIN error)
  - Portal's `selection_read()` returns a non-blocking pipe FD
  - Now sets FD to blocking mode using fcntl before reading
  - Uses `spawn_blocking` for proper blocking I/O on tokio threadpool
  - Fixes Linux→Windows image clipboard copy which was failing with "Resource temporarily unavailable (os error 11)"

## [0.2.1] - 2025-12-23

### Fixed
- **CRITICAL:** Fixed FD ownership issue causing PipeWire stream to close prematurely
- Changed return type from `OwnedFd` to `RawFd` with `std::mem::forget()` to prevent auto-close
- Fixes black screen bug where PipeWire stream stuck in Connecting state

### Added
- Enhanced debug logging for Portal session startup

## [0.2.0] - 2025-12-21

### Added
- `dbus-clipboard` feature for D-Bus clipboard integration (GNOME fallback when Portal clipboard unavailable)

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

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-portal-v0.2.0...HEAD
[0.2.0]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-portal-v0.1.2...lamco-portal-v0.2.0
[0.1.2]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-portal-v0.1.1...lamco-portal-v0.1.2
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-portal-v0.1.1
