//! Configuration example
//!
//! This example demonstrates:
//! - Customizing Portal configuration with builder pattern
//! - Setting cursor mode, persistence, and source types
//! - Using struct literal configuration
//!
//! Run with: cargo run --example config

use ashpd::desktop::remote_desktop::DeviceType;
use ashpd::desktop::screencast::{CursorMode, SourceType};
use ashpd::desktop::PersistMode;
use lamco_portal::PortalManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use lamco_portal::PortalConfig;
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    println!("=== lamco-portal Configuration Example ===\n");

    // Example 1: Using defaults
    println!("1. Creating manager with default configuration:");
    let manager1 = PortalManager::with_default().await?;
    println!("   ✓ Default config: cursor=Metadata, persist=DoNot, multi-monitor=true\n");
    manager1.cleanup().await?;

    // Example 2: Using builder pattern
    println!("2. Creating manager with builder pattern:");
    let config2 = PortalConfig::builder()
        .cursor_mode(CursorMode::Embedded) // Embed cursor in video stream
        .persist_mode(PersistMode::Application) // Remember permission
        .source_type(SourceType::Monitor.into()) // Only monitors (no windows)
        .allow_multiple(false) // Single monitor only
        .build();

    let manager2 = PortalManager::new(config2).await?;
    println!("   ✓ Custom config: cursor=Embedded, persist=Application, single-monitor\n");
    manager2.cleanup().await?;

    // Example 3: Using struct literal
    println!("3. Creating manager with struct literal:");
    let config3 = PortalConfig {
        cursor_mode: CursorMode::Hidden,              // No cursor in stream
        persist_mode: PersistMode::ExplicitlyRevoked, // Remember until revoked
        source_type: SourceType::Window.into(),       // Only windows (no monitors)
        devices: DeviceType::Keyboard.into(),         // Keyboard only, no pointer
        allow_multiple: true,
        restore_token: None,
    };

    let manager3 = PortalManager::new(config3).await?;
    println!("   ✓ Custom config: cursor=Hidden, windows-only, keyboard-only\n");
    manager3.cleanup().await?;

    // Example 4: Monitor-only capture with embedded cursor
    println!("4. Creating session with embedded cursor (monitor-only):");
    let config4 = PortalConfig::builder()
        .cursor_mode(CursorMode::Embedded)
        .source_type(SourceType::Monitor.into())
        .build();

    let manager4 = PortalManager::new(config4).await?;
    println!("   Creating session (permission dialog will appear)...");
    let session = manager4.create_session("config-example".to_string(), None).await?;

    println!("   ✓ Session created with {} streams", session.streams().len());
    println!("\n   Configuration notes:");
    println!("   - Cursor is embedded in the video stream");
    println!("   - Only monitors are available for selection");
    println!("   - Permission will be requested each time (DoNot persist)");

    println!("\nPress Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    manager4.cleanup().await?;

    println!("\n=== Configuration Options ===");
    println!("CursorMode:");
    println!("  - Hidden: No cursor in stream");
    println!("  - Embedded: Cursor baked into video");
    println!("  - Metadata: Cursor position as metadata (recommended for RDP)");
    println!("\nPersistMode:");
    println!("  - DoNot: Request permission every time");
    println!("  - Application: Remember for this app");
    println!("  - ExplicitlyRevoked: Remember until user revokes");
    println!("\nSourceType:");
    println!("  - Monitor: Physical monitors");
    println!("  - Window: Individual windows");
    println!("  - Monitor | Window: Both (default)");

    Ok(())
}
