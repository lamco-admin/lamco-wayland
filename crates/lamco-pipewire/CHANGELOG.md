# Changelog

All notable changes to lamco-pipewire will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.11] - 2026-08-29

### Added
- `PipeWireThreadManager::corrupted_buffer_count()` reports how many buffers the
  producer has marked `SPA_CHUNK_FLAG_CORRUPTED` since the manager started.
  Corrupted buffers usually carry `chunk->size == 0` and are dropped before a
  `VideoFrame` is built, so a consumer that only watches the frame channel
  cannot distinguish "compositor has nothing new to send" from "compositor is
  producing nothing but corrupted buffers" (the signature of the Mutter
  direct-scanout screencast freeze, GNOME/mutter#3903). Sampling this counter
  separates the two.

### Changed
- The `SPA_CHUNK_FLAG_CORRUPTED` warning is now rate limited: the first
  occurrence and then every hundredth, each carrying the running total. A
  compositor stuck in direct scanout flags its whole buffer pool at frame rate
  (978 in a 63 second session on GNOME 50.4), which previously drowned the log.

Ported from 0.6.11 on `master`; the two lines carry this code identically.

## [0.5.10] - 2026-08-26

### Changed
- **Stream timing now goes through pipewire-rs's own safe `Stream::time()`**
  (added in pipewire-rs 0.10.0 via upstream MR !268) instead of an in-crate
  unsafe wrapper around raw `pw_stream_get_time_n()`. No change to this
  crate's own public `StreamTime` / `get_stream_time` surface; downstream
  consumers are unaffected. No `Cargo.toml` feature change needed on this
  branch: the existing `v0_3_65` feature on the `pipewire` dependency already
  cascades down to `v0_3_50`, which `Stream::time()` needs internally.

## [0.4.5] - 2026-06-14

### Fixed
- **Damage detection on aarch64 (NEON)**: the NEON pixel-comparison kernel summed
  the per-byte "exceeds threshold" mask with an 8-bit horizontal add
  (`vaddvq_u8`), which overflowed once more than one byte in a 16-byte chunk
  changed. A fully-changed tile reduced to zero, so changed regions reported no
  damage and incremental encoding silently broke on arm64. The kernel now reduces
  one count per BGRA pixel (alpha ignored) with a 32-bit horizontal add that
  cannot overflow, matching the scalar reference exactly.
- **AVX2 threshold comparison**: replaced the signed `_mm256_cmpgt_epi8` (wrong
  once a channel difference exceeds 127) with an unsigned saturating-subtract
  test, and switched to the same per-pixel reduction. Removes the previous
  byte-count/3 approximation; results now match the scalar path.

### Added
- Property test asserting the dispatched SIMD path (AVX2/NEON/scalar) equals the
  scalar reference across varied buffer lengths and thresholds.

## [0.4.4] - 2026-06-09

### Fixed
- **DMA-BUF modifier negotiation**: advertise `DRM_FORMAT_MOD_LINEAR` instead of
  `DRM_FORMAT_MOD_INVALID`. This consumer reads buffers with a plain CPU mmap,
  which can only interpret row-major linear layouts; "any modifier" invited
  tiled or host-resident allocations that read back as garbage on real GPUs and
  all-zeros on virtio-gpu. Producers that cannot supply linear skip the
  MANDATORY DmaBuf pod and fall through to the SHM/MemFd fallback.
- **Negotiated-modifier guard**: the process callback stores the modifier
  fixated in `param_changed` and refuses to CPU-read a DMA-BUF whose layout is
  not linear (throttled error, frame skipped). The passthrough descriptor now
  carries the negotiated modifier instead of a hardcoded 0.
- **Direct-frame-adapter shutdown gate**: the adapter thread blocked on
  `recv()` until the upstream sender dropped, which never happened on SIGINT —
  the process kept running for minutes after the visible shutdown sequence.
  The adapter now polls a shutdown flag on a 250ms `recv_timeout` loop and the
  manager notifies it on shutdown. Also adds rate-limited drop warnings and a
  10-second ingress heartbeat.
- **DestroyStream reentrancy**: DMA-BUF cache cleanup uses `try_borrow_mut`
  instead of `borrow_mut`, preventing a `BorrowMutError` panic when PipeWire
  dispatches reentrantly during stream destruction (observed on resize).

### Changed
- Per-frame capture logging demoted from `info` to `trace`/`debug`. The
  process/buffer path logged 7+ info lines per captured frame (200+ journal
  lines per second at 30fps). First-frames analysis and one-time negotiation
  logging keep their levels.

## [0.4.3] - 2026-06-03

### Fixed
- The YUV `convert_to_bgra` and damage `detect` entry points no longer panic or
  over-read on malformed frames (size mismatch, oversized dimensions, or odd
  dimensions for 4:2:0). Found by fuzzing; added fuzz targets and regression
  tests.

## [0.4.2] - 2026-03-28

### Fixed
- **DMA-BUF sync ioctl constant**: Corrected `DMA_BUF_IOCTL_SYNC` from `0x40086201`
  to `0x40086200`. The previous value was `DMA_BUF_SET_NAME`, not `SYNC`.

## [0.4.1] - 2026-03-28

### Fixed
- **DMA-BUF mmap returns all zeros**: Added `DMA_BUF_IOCTL_SYNC` before and after
  reading mmap'd GPU buffers. Without the sync ioctl, CPU cache coherency is not
  guaranteed and reads return uninitialized data on GPU-rendered frames.

## [0.4.0] - 2026-03-26

### Added
- **DMA-BUF zero-copy types**: `FrameBuffer` enum with `Memory` and `DmaBuf` variants
- `DmaBufDescriptor` and `DmaBufPlane` types with `OwnedFd` for GPU buffer passthrough
- `dmabuf_passthrough` config flag for opting into zero-copy frame delivery
- `VideoFrame::data()` accessor method for backward-compatible CPU pixel access
- `VideoFrame::is_dmabuf()` convenience check

### Changed
- **Breaking**: `VideoFrame.data: Arc<Vec<u8>>` replaced with `VideoFrame.buffer: FrameBuffer`
- Consumers accessing pixel data should use `frame.data()` (returns `Option<&Arc<Vec<u8>>>`)
  or match on `frame.buffer` directly for DMA-BUF handling

### Fixed
- Pre-existing clippy warnings: added safety comments to unsafe blocks in process callback
- Replaced deprecated `map_or` with `is_none_or` in damage detector

## [0.3.3] - 2026-03-15

### Changed
- Bump to Rust edition 2024, minimum supported Rust version 1.85

### Fixed
- **DmaBuf format negotiation restored**: The dual-pod MANDATORY|DONT_FIXATE
  pattern (introduced in 0.1.6) was lost during the 0.3.x rewrite when
  `build_stream_parameters()` was simplified from `Vec<Vec<u8>>` to `Vec<u8>`.
  Restored proper negotiation: first pod with VideoModifier and MANDATORY|DONT_FIXATE
  for DmaBuf, second pod as SHM fallback without modifier.
- Enabled `v0_3_33` feature on pipewire/libspa deps for `PropertyFlags::DONT_FIXATE`

## [0.3.2] - 2026-03-12

### Fixed

- **SIGSEGV on MemFd buffer copy**: PipeWire's MAP_BUFFERS auto-mapping can produce
  stale pointers for MemFd buffers received via portal FD connections (observed with
  XDPH on PipeWire 1.6.1). MemFd handler now always uses manual `mmap_fd_buffer()`
  instead of relying on `data.data()`. Affects any portal backend that provides MemFd
  buffers (not compositor-specific).
- Fixed clippy `similar_names` warning in process callback variable naming

## [0.3.1] - 2026-03-08

### Fixed
- Add DRIVER stream flag for proper PipeWire scheduling
- Parse negotiated format from `param_changed` callback for accurate buffer handling

## [0.3.0] - 2026-03-04

### Changed
- **BREAKING**: Upgrade to PipeWire 0.9 / libspa 0.9 bindings
- **BREAKING**: Upgrade to zbus 5 for D-Bus integration
- StreamTime FFI improvements
- Audio capture support (behind `audio` feature)
- Direct frame channel adapter for non-PipeWire capture paths

## [0.2.0] - 2026-02-26

### Changed

- **BREAKING**: Public API now takes `OwnedFd` instead of `RawFd`
  - `PipeWireThreadManager::new(fd: OwnedFd)` (was `RawFd`)
  - `PipeWireConnection::new(fd: OwnedFd)` (was `RawFd`)
  - `PipeWireManager::connect(&mut self, fd: OwnedFd)` (was `RawFd`)
  - `PipeWireConnection::fd()` now returns `Option<RawFd>` (None after connect consumes it)
- Removed all internal `unsafe { OwnedFd::from_raw_fd() }` — callers own the FD from the start
- Internal buffer FDs remain `RawFd` (borrowed from PipeWire, not owned by us)

### Migration

Callers that previously passed a raw integer now pass an `OwnedFd`:

```rust
// Before (0.1.x)
manager.connect(portal_fd).await?;

// After (0.2.0)
use std::os::fd::OwnedFd;
manager.connect(owned_fd).await?;
```

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

[Unreleased]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.2.0...HEAD
[0.2.0]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.6...lamco-pipewire-v0.2.0
[0.1.6]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.5...lamco-pipewire-v0.1.6
[0.1.5]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.4...lamco-pipewire-v0.1.5
[0.1.4]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.3...lamco-pipewire-v0.1.4
[0.1.3]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.2...lamco-pipewire-v0.1.3
[0.1.2]: https://github.com/lamco-admin/lamco-wayland/compare/lamco-pipewire-v0.1.1...lamco-pipewire-v0.1.2
[0.1.1]: https://github.com/lamco-admin/lamco-wayland/releases/tag/lamco-pipewire-v0.1.1
