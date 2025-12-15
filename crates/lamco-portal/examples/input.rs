//! Input injection example
//!
//! This example demonstrates:
//! - Creating a Portal session with input capabilities
//! - Injecting mouse movements and clicks
//! - Injecting keyboard events
//!
//! Run with: cargo run --example input
//!
//! SAFETY: This example will move your mouse and simulate clicks!
//! Make sure you're ready before running it.

use lamco_portal::PortalManager;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    println!("=== lamco-portal Input Injection Example ===\n");
    println!("⚠️  WARNING: This example will move your mouse and simulate clicks!");
    println!("⚠️  You have 3 seconds to cancel (Ctrl+C)...\n");
    sleep(Duration::from_secs(3)).await;

    // Create portal manager
    println!("Creating Portal manager...");
    let manager = PortalManager::with_default().await?;
    println!("✓ Portal manager created\n");

    // Create session (triggers permission dialog)
    println!("Creating session (permission dialog will appear)...");
    println!("Make sure to grant BOTH screen capture AND input control permissions!\n");
    let session = manager.create_session("input-example".to_string(), None).await?;
    println!("✓ Session created\n");

    // Get the first stream for pointer positioning
    let stream_index = 0;
    if session.streams().is_empty() {
        eprintln!("No streams available!");
        return Ok(());
    }

    println!("Demonstrating input injection...\n");

    // Example 1: Move mouse to center of screen
    println!("1. Moving mouse to screen center...");
    manager
        .remote_desktop()
        .notify_pointer_motion_absolute(
            session.ashpd_session(),
            stream_index,
            0.5, // 50% x (center)
            0.5, // 50% y (center)
        )
        .await?;
    sleep(Duration::from_secs(1)).await;

    // Example 2: Move to top-left corner
    println!("2. Moving mouse to top-left corner...");
    manager
        .remote_desktop()
        .notify_pointer_motion_absolute(
            session.ashpd_session(),
            stream_index,
            0.1, // 10% x
            0.1, // 10% y
        )
        .await?;
    sleep(Duration::from_secs(1)).await;

    // Example 3: Move to bottom-right corner
    println!("3. Moving mouse to bottom-right corner...");
    manager
        .remote_desktop()
        .notify_pointer_motion_absolute(
            session.ashpd_session(),
            stream_index,
            0.9, // 90% x
            0.9, // 90% y
        )
        .await?;
    sleep(Duration::from_secs(1)).await;

    // Example 4: Simulate a left click (press and release)
    println!("4. Simulating left mouse click...");
    // Button 1 = left mouse button
    manager
        .remote_desktop()
        .notify_pointer_button(session.ashpd_session(), 1, true) // Press
        .await?;
    sleep(Duration::from_millis(100)).await;
    manager
        .remote_desktop()
        .notify_pointer_button(session.ashpd_session(), 1, false) // Release
        .await?;
    sleep(Duration::from_secs(1)).await;

    // Example 5: Keyboard input (simulate pressing 'A' key)
    println!("5. Simulating 'A' key press...");
    // Keycode 30 = 'A' key (Linux keycode)
    manager
        .remote_desktop()
        .notify_keyboard_keycode(session.ashpd_session(), 30, true) // Press
        .await?;
    sleep(Duration::from_millis(100)).await;
    manager
        .remote_desktop()
        .notify_keyboard_keycode(session.ashpd_session(), 30, false) // Release
        .await?;

    println!("\n✓ Input injection demonstration complete!");
    println!("\nNOTE: In a real application, you would:");
    println!("  - Get mouse coordinates from your remote desktop protocol");
    println!("  - Convert protocol keycodes to Linux keycodes");
    println!("  - Handle button states properly");

    manager.cleanup().await?;

    Ok(())
}
