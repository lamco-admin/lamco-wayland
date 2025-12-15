//! Basic lamco-video usage example
//!
//! This example demonstrates the basic components of lamco-video:
//! - ProcessorConfig for configuring frame processing
//! - BitmapConverter for pixel format conversion
//! - Rectangle and damage region handling
//!
//! Note: This example doesn't process actual frames - it just
//! demonstrates the API. For actual frame processing, you need
//! lamco-pipewire to capture frames from PipeWire.

use lamco_video::{
    BitmapConverter, DispatcherConfig, ProcessorConfig, RdpPixelFormat, Rectangle,
};

fn main() {
    println!("lamco-video v{}", lamco_video::VERSION);
    println!();

    // Demonstrate processor configuration
    let processor_config = ProcessorConfig {
        target_fps: 60,
        max_queue_depth: 30,
        adaptive_quality: true,
        damage_threshold: 0.05,
        drop_on_full_queue: true,
        enable_metrics: true,
    };
    println!("Processor Config:");
    println!("  Target FPS: {}", processor_config.target_fps);
    println!("  Max queue depth: {}", processor_config.max_queue_depth);
    println!("  Adaptive quality: {}", processor_config.adaptive_quality);
    println!();

    // Demonstrate dispatcher configuration
    let dispatcher_config = DispatcherConfig::default();
    println!("Dispatcher Config (default):");
    println!("  Channel size: {}", dispatcher_config.channel_size);
    println!("  Priority dispatch: {}", dispatcher_config.priority_dispatch);
    println!("  Max frame age: {}ms", dispatcher_config.max_frame_age_ms);
    println!("  Backpressure enabled: {}", dispatcher_config.enable_backpressure);
    println!();

    // Demonstrate bitmap converter creation
    let converter = BitmapConverter::new(1920, 1080);
    println!("BitmapConverter created for 1920x1080");
    let stats = converter.get_statistics();
    println!("  Frames converted: {}", stats.frames_converted);
    println!("  Bytes processed: {}", stats.bytes_processed);
    println!();

    // Demonstrate RDP pixel formats
    println!("RDP Pixel Formats:");
    for format in [
        RdpPixelFormat::BgrX32,
        RdpPixelFormat::Bgr24,
        RdpPixelFormat::Rgb16,
        RdpPixelFormat::Rgb15,
    ] {
        println!("  {:?}: {} bytes/pixel", format, format.bytes_per_pixel());
    }
    println!();

    // Demonstrate rectangle operations
    let rect1 = Rectangle::new(0, 0, 100, 100);
    let rect2 = Rectangle::new(50, 50, 150, 150);
    println!("Rectangle operations:");
    println!("  rect1: ({}, {}) to ({}, {})", rect1.left, rect1.top, rect1.right, rect1.bottom);
    println!("  rect1 area: {} pixels", rect1.area());
    println!("  rect2: ({}, {}) to ({}, {})", rect2.left, rect2.top, rect2.right, rect2.bottom);
    println!("  Intersects: {}", rect1.intersects(&rect2));
    println!();

    // Demonstrate helper functions
    println!("Helper functions:");
    println!(
        "  Recommended queue size for 60Hz: {}",
        lamco_video::recommended_queue_size(60)
    );
    println!(
        "  Recommended queue size for 144Hz: {}",
        lamco_video::recommended_queue_size(144)
    );
    println!(
        "  RDP stride for 1920px @ BgrX32: {} bytes",
        lamco_video::calculate_rdp_stride(1920, RdpPixelFormat::BgrX32)
    );
}
