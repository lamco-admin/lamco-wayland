# Changelog

All notable changes to the lamco-wayland workspace will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-06-30

### Changed
- **lamco-pipewire 0.6.0: metadata extraction rewritten over libspa 0.10's safe
  wrappers.** `meta.rs` now reads SPA_META_Header / VideoTransform / VideoCrop /
  VideoDamage / Cursor through `Buffer::find_meta::<T>()` instead of raw
  `libspa_sys` pointer access — the module is now `unsafe`-free, and the
  hand-walked damage-region array (the source of the earlier aarch64 overflow
  bug) is replaced by the wrapper's iterator.
  - The process callback now uses the safe `Stream::dequeue_buffer()` → `Buffer`
    (which requeues itself on drop) and `Buffer::datas_mut()`, replacing
    `dequeue_raw_buffer` + a manual `pw_buffer` queue guard + `from_raw_parts_mut`.
  - `extract_buffer_meta` is now a safe `fn(&Buffer)` (was
    `unsafe fn(*const spa_buffer)`).
  - libspa feature raised `v0_3_33` → `v0_3_62` for the typed `MetaVideoTransform`
    wrapper (raises the system-library floor to libpipewire 0.3.62).
  - `ffi::SpaDataType` removed in favor of `ffi::DataType` (re-exported
    `libspa::buffer::DataType`); `BufferType::from_spa_type` now takes `DataType`.

### Added
- `CursorMeta::bitmap` (new `CursorBitmap` with `format` / `width` / `height` /
  `stride` / `pixels`): the actual cursor image, decoded via
  `MetaCursor::bitmap()` / `MetaBitmap::bitmap_data()` when the compositor
  attaches one (cursor-change frames). Previously only `bitmap_offset` was
  exposed. Serves the `cursor` feature.

### Note
- Sub-crate versions: lamco-pipewire 0.6.0, lamco-video 0.3.0 — both minor
  (breaking) bumps for the public-API changes above.

## [0.5.0] - 2026-06-30

### Changed
- **PipeWire / SPA Rust bindings upgraded from 0.9 to 0.10** (`pipewire`,
  `pipewire-sys`, `libspa`, `libspa-sys`). The `v0_3_33` feature flag and all
  capture behavior are unchanged — this is a clean version migration; 0.10's
  new safe metadata wrappers are adopted in a follow-up release.
  - `Loop::iterate` now takes a `Timeout` enum instead of a `Duration`: the
    non-blocking 0 ms poll in the capture loop is now `Timeout::None`, and the
    connection (10 ms) and audio (100 ms) loops use `Timeout::Finite(..)`.
  - Sub-crate versions: lamco-pipewire 0.4.5 → 0.5.0, lamco-video 0.1.10 →
    0.2.0. Both are minor (breaking) bumps because the 0.10 bindings appear in
    their public APIs.
- MSRV is unchanged (1.87); pipewire 0.10 requires only Rust 1.80.

### Fixed
- Advisory RUSTSEC-2026-0190: bumped the transitive `anyhow` to 1.0.103
  (Stacked Borrows UB in `Error::downcast_mut`, reachable only via the
  `audio` feature).

## [0.4.7] - 2026-06-14

### Fixed
- lamco-pipewire 0.4.5: damage detection on aarch64 (NEON) no longer reports zero
  damage for fully-changed tiles (8-bit horizontal-add overflow); AVX2 threshold
  comparison corrected. Detection now matches the scalar reference on all targets.

### Changed
- Updated lamco-pipewire to 0.4.5 and lamco-video to 0.1.10.

## [0.4.6] - 2026-06-09

### Changed
- Updated lamco-pipewire to 0.4.4
- lamco-pipewire 0.4.4: per-frame capture logging demoted from info to
  trace/debug (was ~7 info lines per frame — 200+ journal lines/sec at 30fps).

### Fixed
- lamco-pipewire 0.4.4: DMA-BUF capture negotiates `DRM_FORMAT_MOD_LINEAR`
  instead of `DRM_FORMAT_MOD_INVALID`. The CPU-mmap consume path can only read
  row-major linear buffers; advertising "any modifier" let compositors fixate
  tiled or host-resident layouts that read back as garbage on real GPUs and
  all-zeros on virtio-gpu (issue #5). Producers that cannot supply linear fall
  through to the existing SHM/MemFd param automatically.
- lamco-pipewire 0.4.4: the process callback now checks the negotiated modifier
  before CPU-reading a DMA-BUF and skips non-linear frames with a throttled
  error instead of delivering garbage; the passthrough descriptor carries the
  negotiated modifier instead of a hardcoded 0.
- lamco-pipewire 0.4.4: direct-frame-adapter thread now honors shutdown — it
  previously blocked on `recv()` until upstream closed, leaving a zombie
  process for minutes after SIGINT. Also adds drop-rate warnings and a 10s
  ingress heartbeat.
- lamco-pipewire 0.4.4: DestroyStream cleanup uses `try_borrow_mut` on the
  DMA-BUF mmap cache, preventing a `BorrowMutError` panic on reentrant
  PipeWire dispatch (resize path).

## [0.4.5] - 2026-06-03

### Changed
- Relicensed to Lamco Development LLC; removed `authors` metadata.
- Workspace MSRV raised to 1.87 to match ashpd 0.13 / zbus 5.
- `thiserror` updated from 1.0 to 2.0 across all crates (removes a duplicate dep).
- CI hardened: clippy `--all-targets` in both default and all-features modes,
  cargo-deny, a 1.87 MSRV gate, a fuzz-smoke job (with PipeWire system deps), and
  THIRD_PARTY_NOTICES.
- Sub-crate versions: lamco-portal 0.4.1, lamco-pipewire 0.4.3, lamco-video 0.1.9.

### Fixed
- lamco-pipewire 0.4.3: the YUV `convert_to_bgra` and damage `detect` entry points
  no longer panic or over-read on malformed frames (size mismatch, oversized
  dimensions, or odd dimensions for 4:2:0). Found by fuzzing; added fuzz targets
  and regression tests.
- Advisory RUSTSEC-2026-0007: bumped the transitive `bytes` to 1.11.1.

## [0.4.4] - 2026-03-28

### Changed
- Updated lamco-pipewire to 0.4.2 (corrected DMA-BUF sync ioctl constant)

## [0.4.3] - 2026-03-28

### Changed
- Updated lamco-pipewire to 0.4.1 (DMA-BUF sync fix for GPU buffer readback)

## [0.3.1] - 2026-03-15

### Changed
- Updated lamco-portal to 0.3.4 (ashpd 0.13.7 upgrade, Session::Closed fix)

## [0.3.0] - 2026-03-15

### Changed
- Bump to Rust edition 2024, minimum supported Rust version 1.85
- Updated all sub-crate dependencies:
  - lamco-pipewire 0.3.3 (DmaBuf negotiation fix, BufferMeta, edition 2024)
  - lamco-portal 0.3.3 (zbus 5, edition 2024)
  - lamco-video 0.1.5 (edition 2024)

## [0.2.9] - 2026-03-12

### Changed
- Updated lamco-pipewire to 0.3.2 (MemFd SIGSEGV fix)

## [0.2.8] - 2026-03-08

### Changed
- Updated lamco-pipewire to 0.3.1 (DRIVER stream flag, format parsing)

## [0.2.7] - 2026-03-04

### Changed
- Updated all dependencies for PipeWire 0.9, zbus 5, edition upgrades
- lamco-pipewire 0.3.0, lamco-portal 0.3.2, lamco-video 0.1.4

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
