# lamco-pipewire Publication Plan

## Current Status: Implementation Complete

All planned features have been implemented. **74 unit tests + 6 doc tests passing.**

---

## Completion Summary

### Phase 1: Foundation - COMPLETE

| Task | Status | Notes |
|------|--------|-------|
| Linting alignment | Done | Workspace-compatible lints in Cargo.toml |
| Config builder pattern | Done | `PipeWireConfig::builder()` API |
| Unified PipeWireManager | Done | Single entry point with thread abstraction |

### Phase 2: Features - COMPLETE

| Feature | Status | Module | Notes |
|---------|--------|--------|-------|
| DMA-BUF support | Done | (default) | Zero-copy frame transfer |
| YUV conversion | Done | `src/yuv.rs` | NV12, I420, YUY2 to BGRA |
| Cursor extraction | Done | `src/cursor.rs` | Hardware cursor with stats |
| Damage tracking | Done | `src/damage.rs` | Region-based change detection |
| Adaptive bitrate | Done | `src/bitrate.rs` | Network-aware bitrate control |

### Phase 3: Polish - COMPLETE

| Task | Status | Notes |
|------|--------|-------|
| Documentation | Done | Comprehensive lib.rs with architecture diagram |
| Examples | Done | 4 examples created |
| String renaming | Done | "wrd-capture" → "lamco-pw" |
| Test fixes | Done | All tests passing |

---

## Feature Flags

```toml
[features]
default = ["dmabuf"]
dmabuf = []      # DMA-BUF zero-copy support
yuv = []         # YUV format conversion utilities
cursor = []      # Hardware cursor extraction
damage = []      # Region damage tracking
adaptive = []    # Adaptive bitrate helpers
full = ["dmabuf", "yuv", "cursor", "damage", "adaptive"]
```

### Feature Descriptions

#### `dmabuf` (default)
Zero-copy frame transfer using DMA-BUF file descriptors. When available, frames are passed directly from the compositor's GPU buffer to your application without CPU-side memory copies. This is the primary performance optimization for screen capture.

**Use case:** Any screen capture application wanting best performance.

#### `yuv`
YUV to RGB color format conversion utilities. PipeWire may provide frames in compressed YUV formats (NV12, I420, YUY2) depending on the compositor and hardware encoder. These utilities convert to BGRA for display or further processing.

**Functions:**
- `nv12_to_bgra()` - YUV 4:2:0 with interleaved UV
- `i420_to_bgra()` - YUV 4:2:0 with separate U/V planes
- `yuy2_to_bgra()` - YUV 4:2:2 packed
- `YuvConverter` - Format-detecting converter

**Use case:** Applications that receive YUV frames and need RGB for display/encoding.

#### `cursor`
Hardware cursor extraction from PipeWire streams. The compositor provides cursor metadata separately from the frame buffer, allowing efficient cursor handling without re-encoding the entire frame.

**Types:**
- `CursorInfo` - Position, hotspot, size, bitmap, visibility
- `CursorExtractor` - Stateful extractor with caching
- `CursorStats` - Update/change tracking

**Use case:** Remote desktop applications that need to track/transmit cursor separately.

#### `damage`
Region-based change detection. PipeWire can report which regions of the screen changed between frames, allowing partial updates instead of full-frame encoding.

**Types:**
- `DamageRegion` - Rectangle with x, y, width, height
- `DamageTracker` - Accumulates and merges damage regions
- `DamageStats` - Region count and area tracking

**Methods:**
- `should_full_update()` - Returns true if damage exceeds threshold
- `bounding_box()` - Single rectangle encompassing all damage
- `damage_ratio()` - Fraction of frame that changed

**Use case:** Video encoders that support partial updates (H.264 ROI, etc.)

#### `adaptive`
Network-aware bitrate control for streaming scenarios. Tracks frame encoding times and network feedback to recommend bitrate adjustments.

**Types:**
- `BitrateController` - Main controller with recommendation logic
- `BitrateStats` - Frames recorded, dropped, skipped
- `QualityPreset` - LowLatency, Balanced, HighQuality

**Methods:**
- `record_frame()` - Log encode time and frame size
- `record_network_feedback()` - Log packet loss and RTT
- `recommended_bitrate()` - Current recommended bitrate
- `should_skip_frame()` - True if congestion detected

**Use case:** Streaming applications that need to adapt to network conditions.

---

## Files Created

### New Modules
| File | Purpose |
|------|---------|
| `src/config.rs` | Configuration structs and builders |
| `src/manager.rs` | Unified PipeWireManager API |
| `src/yuv.rs` | YUV format conversion |
| `src/cursor.rs` | Hardware cursor extraction |
| `src/damage.rs` | Region damage tracking |
| `src/bitrate.rs` | Adaptive bitrate control |

### Examples
| File | Purpose |
|------|---------|
| `examples/basic.rs` | Simple single-stream capture |
| `examples/multi_monitor.rs` | Multi-monitor coordination |
| `examples/with_damage.rs` | Damage tracking demonstration |
| `examples/streaming.rs` | Adaptive bitrate for streaming |

### Modified Files
- `Cargo.toml` - Lints, features, dependencies
- `src/lib.rs` - Documentation, module declarations, re-exports
- `src/stream.rs` - Renamed "wrd-capture" → "lamco-pw"
- `src/pw_thread.rs` - Renamed "wrd-capture" → "lamco-pw"
- `src/buffer.rs` - Fixed unsafe block warnings

---

## Test Results

```
test result: ok. 74 passed; 0 failed; 1 ignored; 0 measured

Doc-tests:
test result: ok. 6 passed; 0 failed; 8 ignored; 0 measured
```

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│              Tokio Async Runtime                        │
│                                                         │
│  Your Application → PipeWireManager                     │
│                    (Send + Sync wrapper)                │
│                           │                             │
│                           │ Commands via mpsc           │
│                           ▼                             │
└───────────────────────────┼─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│         Dedicated PipeWire Thread                       │
│         (std::thread - owns all non-Send types)         │
│                                                         │
│  MainLoop (Rc) ─> Context (Rc) ─> Core (Rc)            │
│                                      │                  │
│                                      ▼                  │
│                              Streams (NonNull)          │
│                                      │                  │
│                                      ▼                  │
│                              Frame Callbacks            │
│                                      │                  │
│                                      │ Frames via mpsc  │
└──────────────────────────────────────┼──────────────────┘
                                       │
                                       ▼
                             Your application receives frames
```

---

## Outstanding Questions for Discussion

1. **Feature granularity**: Are these the right feature boundaries? Should any be combined or split further?

2. **Default features**: Currently only `dmabuf` is default. Should `yuv` be default since YUV frames are common?

3. **Performance vs. correctness**: The YUV conversion is reference implementation (correct but not optimized). Should we add SIMD or note this more prominently?

4. **Cursor feature scope**: Currently basic extraction. Should it include cursor shape caching across sessions?

5. **Damage tracking threshold**: Default is 40% for full-frame fallback. Is this the right default?

6. **Adaptive bitrate presets**: Are LowLatency/Balanced/HighQuality the right preset names?

---

## Next Steps

- [ ] Feature discussion with maintainer
- [ ] Review API surface for publication
- [ ] Final documentation review
- [ ] Publish to crates.io
