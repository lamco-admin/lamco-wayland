//! Basic PipeWire Screen Capture Example
//!
//! This example demonstrates the simplest usage of lamco-pipewire
//! for capturing screen content from a single monitor.
//!
//! # Prerequisites
//!
//! - PipeWire must be installed and running
//! - You need a portal-provided file descriptor (typically from lamco-portal)
//! - This example uses a mock FD for demonstration
//!
//! # Running
//!
//! ```bash
//! cargo run --example basic
//! ```

use lamco_pipewire::{
    PipeWireConfig, PipeWireManager, PixelFormat, SourceType, StreamInfo,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("lamco-pipewire Basic Example");
    println!("============================");

    // Create manager with custom configuration
    let config = PipeWireConfig::builder()
        .buffer_count(3)
        .preferred_format(PixelFormat::BGRA)
        .use_dmabuf(true)
        .max_streams(4)
        .build();

    println!("Configuration:");
    println!("  Buffer count: {}", config.buffer_count);
    println!("  Preferred format: {:?}", config.preferred_format);
    println!("  Use DMA-BUF: {}", config.use_dmabuf);
    println!("  Max streams: {}", config.max_streams);

    let manager = PipeWireManager::new(config)?;
    println!("\nManager created successfully!");
    println!("State: {:?}", manager.state().await);

    // In a real application, you would:
    // 1. Use lamco-portal to get a PipeWire file descriptor
    // 2. Call manager.connect(fd).await
    // 3. Create streams for each monitor
    // 4. Receive frames via frame_receiver()
    //
    // Example (requires actual portal session):
    //
    // ```
    // use lamco_portal::PortalManager;
    //
    // let portal = PortalManager::with_default().await?;
    // let session = portal.create_session("example".to_string(), None).await?;
    //
    // manager.connect(session.pipewire_fd()).await?;
    //
    // for stream in session.streams() {
    //     let info = StreamInfo {
    //         node_id: stream.node_id,
    //         position: stream.position,
    //         size: stream.size,
    //         source_type: SourceType::Monitor,
    //     };
    //     let handle = manager.create_stream(&info).await?;
    //     println!("Created stream: {}", handle.id);
    // }
    // ```

    // For this example, we'll just show the manager is working
    println!("\nTo use this in a real application:");
    println!("1. Add lamco-portal as a dependency");
    println!("2. Create a portal session to get a PipeWire FD");
    println!("3. Connect the manager to the FD");
    println!("4. Create streams for each monitor");
    println!("5. Receive frames via frame_receiver()");

    // Demonstrate StreamInfo structure
    let _demo_stream_info = StreamInfo {
        node_id: 42,
        position: (0, 0),
        size: (1920, 1080),
        source_type: SourceType::Monitor,
    };

    println!("\nDemo StreamInfo:");
    println!("  Node ID: {}", _demo_stream_info.node_id);
    println!("  Position: {:?}", _demo_stream_info.position);
    println!("  Size: {:?}", _demo_stream_info.size);
    println!("  Source type: {:?}", _demo_stream_info.source_type);

    // Check DMA-BUF support
    println!("\nSystem capabilities:");
    println!(
        "  DMA-BUF likely supported: {}",
        lamco_pipewire::is_dmabuf_supported()
    );

    println!("\nSupported formats:");
    for (i, format) in lamco_pipewire::supported_formats().iter().enumerate() {
        println!("  {}. {:?}", i + 1, format);
    }

    println!("\nExample completed successfully!");
    Ok(())
}
