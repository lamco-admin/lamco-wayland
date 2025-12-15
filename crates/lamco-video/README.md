# lamco-video

Video frame processing and RDP bitmap conversion for Wayland screen capture.

[![Crates.io](https://img.shields.io/crates/v/lamco-video.svg)](https://crates.io/crates/lamco-video)
[![Documentation](https://docs.rs/lamco-video/badge.svg)](https://docs.rs/lamco-video)
[![License](https://img.shields.io/crates/l/lamco-video.svg)](LICENSE-MIT)

## Features

- **Frame Processing Pipeline**: Configurable video frame processing with rate limiting
- **RDP Bitmap Conversion**: Convert PipeWire frames to RDP-ready bitmap format
- **Damage Region Tracking**: Optimize updates by only sending changed regions
- **Buffer Pooling**: Efficient memory management with reusable buffers
- **Priority-Based Dispatch**: Multi-stream coordination with backpressure handling
- **SIMD Optimization**: Automatic use of SIMD instructions where available

## Quick Start

```rust,ignore
use lamco_video::{FrameProcessor, ProcessorConfig, BitmapConverter};
use lamco_pipewire::VideoFrame;
use tokio::sync::mpsc;

// Create frame processor
let config = ProcessorConfig::default();
let processor = std::sync::Arc::new(FrameProcessor::new(config, 1920, 1080));

// Create channels
let (input_tx, input_rx) = mpsc::channel(30);
let (output_tx, mut output_rx) = mpsc::channel(30);

// Start processor
let processor_clone = processor.clone();
tokio::spawn(async move {
    processor_clone.start(input_rx, output_tx).await
});

// Send frames from lamco-pipewire, receive bitmap updates
while let Some(bitmap_update) = output_rx.recv().await {
    for rect in &bitmap_update.rectangles {
        println!("Update region: {:?}", rect.rectangle);
    }
}
```

## Configuration

### Processor Configuration

```rust
use lamco_video::ProcessorConfig;

let config = ProcessorConfig {
    target_fps: 60,           // Target frame rate
    max_queue_depth: 30,      // Max frames in queue before dropping
    adaptive_quality: true,   // Enable adaptive quality
    damage_threshold: 0.05,   // Minimum damage area to process (5%)
    drop_on_full_queue: true, // Drop frames when queue is full
    enable_metrics: true,     // Enable statistics collection
};
```

### Dispatcher Configuration

```rust
use lamco_video::DispatcherConfig;

let config = DispatcherConfig {
    channel_size: 30,          // Buffer size per stream
    priority_dispatch: true,   // Enable priority-based dispatch
    max_frame_age_ms: 150,     // Drop frames older than 150ms
    enable_backpressure: true, // Enable backpressure handling
    high_water_mark: 0.8,      // Trigger backpressure at 80%
    low_water_mark: 0.5,       // Release backpressure at 50%
    load_balancing: true,      // Enable load balancing
};
```

## Architecture

The processing pipeline:

```text
┌────────────────────┐
│  lamco-pipewire    │
│  (VideoFrame)      │
└─────────┬──────────┘
          │
          ▼
┌────────────────────┐
│  FrameDispatcher   │ ◄── Multi-stream routing
│  (priority queue)  │     Backpressure handling
└─────────┬──────────┘
          │
          ▼
┌────────────────────┐
│  FrameProcessor    │ ◄── Frame rate limiting
│  (rate control)    │     Age-based dropping
└─────────┬──────────┘
          │
          ▼
┌────────────────────┐
│  BitmapConverter   │ ◄── Pixel format conversion
│  (format conv)     │     Damage region tracking
└─────────┬──────────┘     Buffer pooling
          │
          ▼
┌────────────────────┐
│  BitmapUpdate      │ ◄── RDP-ready rectangles
│  (RDP output)      │
└────────────────────┘
```

## RDP Pixel Formats

The converter supports these RDP-compatible output formats:

| Format | BPP | Description |
|--------|-----|-------------|
| BgrX32 | 4 | 32-bit BGRX (most common) |
| Bgr24  | 3 | 24-bit BGR |
| Rgb16  | 2 | 16-bit RGB 5:6:5 |
| Rgb15  | 2 | 15-bit RGB 5:5:5 |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `damage` | No | Full damage region tracking |
| `full` | No | All features enabled |

```toml
[dependencies]
lamco-video = { version = "0.1", features = ["full"] }
```

## Performance

Typical performance on modern hardware:

- **Conversion latency**: < 1ms per frame (1080p)
- **Memory usage**: < 50MB (with buffer pooling)
- **Throughput**: > 200 MB/s (with SIMD)
- **Frame rates**: Tested up to 144Hz

## Requirements

- **Linux** with a Wayland compositor
- **Rust 1.77+**

## Related Crates

- [`lamco-portal`](https://crates.io/crates/lamco-portal) - XDG Desktop Portal integration
- [`lamco-pipewire`](https://crates.io/crates/lamco-pipewire) - PipeWire screen capture

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
