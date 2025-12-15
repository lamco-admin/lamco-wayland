//! Multi-Monitor Screen Capture Example
//!
//! This example demonstrates capturing from multiple monitors simultaneously
//! using the PipeWire coordinator.
//!
//! # Prerequisites
//!
//! - PipeWire must be installed and running
//! - Multiple monitors configured (or simulation)
//! - Portal-provided file descriptor
//!
//! # Running
//!
//! ```bash
//! cargo run --example multi_monitor
//! ```

use lamco_pipewire::{MonitorInfo, MultiStreamConfig, PipeWireConfig, PipeWireManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("lamco-pipewire Multi-Monitor Example");
    println!("====================================");

    // Create manager with configuration for multiple streams
    let config = PipeWireConfig::builder()
        .buffer_count(4) // More buffers for multiple streams
        .max_streams(8)
        .frame_buffer_size(60) // Buffer 2 seconds at 30fps
        .build();

    let _manager = PipeWireManager::new(config)?;
    println!("Manager created for multi-monitor capture");

    // Simulate monitor configuration (in real usage, this comes from portal)
    let monitors = vec![
        MonitorInfo {
            id: 0,
            name: "Primary Monitor".to_string(),
            position: (0, 0),
            size: (2560, 1440),
            refresh_rate: 144,
            node_id: 100,
        },
        MonitorInfo {
            id: 1,
            name: "Secondary Monitor".to_string(),
            position: (2560, 0),
            size: (1920, 1080),
            refresh_rate: 60,
            node_id: 101,
        },
        MonitorInfo {
            id: 2,
            name: "Vertical Monitor".to_string(),
            position: (-1080, 180),
            size: (1080, 1920), // Portrait orientation
            refresh_rate: 60,
            node_id: 102,
        },
    ];

    println!("\nDetected {} monitors:", monitors.len());
    for monitor in &monitors {
        println!(
            "  {} (ID: {}): {}x{} @ {}Hz at position {:?}",
            monitor.name, monitor.id, monitor.size.0, monitor.size.1, monitor.refresh_rate, monitor.position
        );
    }

    // Calculate combined resolution
    let total_width: u32 = monitors.iter().map(|m| m.size.0).sum();
    let max_height: u32 = monitors.iter().map(|m| m.size.1).max().unwrap_or(0);
    println!("\nCombined desktop: {}x{}", total_width, max_height);

    // Calculate recommended settings per monitor
    println!("\nRecommended settings per monitor:");
    for monitor in &monitors {
        let buffers = lamco_pipewire::recommended_buffer_count(monitor.refresh_rate);
        let frame_buffer = lamco_pipewire::recommended_frame_buffer_size(monitor.refresh_rate);

        println!(
            "  {}: {} buffers, {} frame buffer",
            monitor.name, buffers, frame_buffer
        );
    }

    // Show MultiStreamConfig defaults
    let multi_config = MultiStreamConfig::default();
    println!("\nMultiStreamConfig defaults:");
    println!("  Max streams: {}", multi_config.max_streams);
    println!("  Enable sync: {}", multi_config.enable_sync);
    println!("  Retry attempts: {}", multi_config.retry_attempts);

    // In a real application, you would connect and create streams:
    //
    // ```
    // manager.connect(fd).await?;
    //
    // for monitor in &monitors {
    //     let info = StreamInfo {
    //         node_id: monitor.node_id,
    //         position: monitor.position,
    //         size: monitor.size,
    //         source_type: SourceType::Monitor,
    //     };
    //
    //     let handle = manager.create_stream(&info).await?;
    //
    //     // Spawn task to process frames for this monitor
    //     let rx = manager.frame_receiver(handle.id).await.unwrap();
    //     tokio::spawn(async move {
    //         while let Some(frame) = rx.recv().await {
    //             // Process frame...
    //         }
    //     });
    // }
    // ```

    println!("\nMulti-monitor architecture:");
    println!("  - Each monitor gets its own PipeWire stream");
    println!("  - Frames are delivered via separate channels");
    println!("  - Frame timestamps enable cross-monitor sync");
    println!("  - Position info enables virtual desktop reconstruction");

    println!("\nExample completed successfully!");
    Ok(())
}
