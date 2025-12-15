# lamco-pipewire Publication Status

## Published

**Version:** 0.1.0
**Date:** 2025-12-15
**Registry:** crates.io

## Links

- **Crate:** https://crates.io/crates/lamco-pipewire
- **Documentation:** https://docs.rs/lamco-pipewire
- **Repository:** https://github.com/lamco-admin/lamco-wayland

## Package Metrics

| Metric | Value |
|--------|-------|
| Files packaged | 26 |
| Uncompressed size | 290 KB |
| Compressed size | 69 KB |
| Unit tests | 74 |
| Doc tests | 6 |
| Warnings | 24 (all unsafe-related, expected) |

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `dmabuf` | Yes | DMA-BUF zero-copy support |
| `yuv` | No | YUV format conversion utilities |
| `cursor` | No | Hardware cursor extraction |
| `damage` | No | Region damage tracking |
| `adaptive` | No | Adaptive bitrate control |
| `full` | No | All features enabled |

## New Modules Created

- `src/config.rs` - Configuration structs and builders
- `src/manager.rs` - Unified PipeWireManager API
- `src/yuv.rs` - YUV format conversion
- `src/cursor.rs` - Hardware cursor extraction
- `src/damage.rs` - Region damage tracking
- `src/bitrate.rs` - Adaptive bitrate control

## Examples

- `examples/basic.rs` - Simple single-stream capture
- `examples/multi_monitor.rs` - Multi-monitor coordination
- `examples/with_damage.rs` - Damage tracking demonstration
- `examples/streaming.rs` - Adaptive bitrate for streaming

## Related Crates

| Crate | Version | Status |
|-------|---------|--------|
| lamco-portal | 0.1.0 | Published |
| lamco-pipewire | 0.1.0 | Published |
| lamco-video | - | Pending |
