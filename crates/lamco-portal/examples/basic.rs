//! Basic screen capture example
//!
//! This example demonstrates:
//! - Creating a Portal manager with default configuration
//! - Creating a session (triggers permission dialog)
//! - Accessing PipeWire FD and stream information
//!
//! Run with: cargo run --example basic

use lamco_portal::PortalManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    println!("=== lamco-portal Basic Example ===\n");

    // Create portal manager with default configuration
    println!("Creating Portal manager...");
    let manager = PortalManager::with_default().await?;
    println!("✓ Portal manager created\n");

    // Create a session (this will trigger the system permission dialog)
    println!("Creating session (permission dialog will appear)...");
    let session = manager.create_session("basic-example".to_string(), None).await?;
    println!("✓ Session created: {}\n", session.session_id());

    // Display PipeWire information
    println!("PipeWire Details:");
    println!("  File Descriptor: {}", session.pipewire_fd());
    println!("  Available Streams: {}\n", session.streams().len());

    // Display stream information
    println!("Stream Information:");
    for (i, stream) in session.streams().iter().enumerate() {
        println!("  Stream {}: ", i);
        println!("    Node ID: {}", stream.node_id);
        println!("    Size: {}x{}", stream.size.0, stream.size.1);
        println!("    Position: ({}, {})", stream.position.0, stream.position.1);
        println!("    Type: {:?}", stream.source_type);
    }

    println!("\nSession active. Press Ctrl+C to exit.");
    println!("(In a real application, you would pass the PipeWire FD to your video capture code)");

    // Keep session alive
    tokio::signal::ctrl_c().await?;

    println!("\nShutting down...");
    manager.cleanup().await?;

    Ok(())
}
