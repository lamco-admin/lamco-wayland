//! Damage Tracking Example
//!
//! This example demonstrates using damage tracking to efficiently
//! encode only the regions of the screen that have changed.
//!
//! # Prerequisites
//!
//! - Requires the `damage` feature
//!
//! # Running
//!
//! ```bash
//! cargo run --example with_damage --features damage
//! ```

#[cfg(feature = "damage")]
use lamco_pipewire::damage::{DamageRegion, DamageTracker};

#[cfg(feature = "damage")]
fn main() {
    println!("lamco-pipewire Damage Tracking Example");
    println!("======================================");

    // Create a damage tracker
    let mut tracker = DamageTracker::with_threshold(0.4);

    // Simulate some damaged regions from PipeWire metadata
    let regions = vec![
        DamageRegion::new(100, 100, 200, 150), // Window update
        DamageRegion::new(500, 300, 50, 30),   // Cursor area
        DamageRegion::new(120, 120, 100, 80),  // Overlapping with first
    ];

    println!("\nAdding {} damage regions...", regions.len());
    for region in &regions {
        println!(
            "  Region: x={}, y={}, {}x{}",
            region.x, region.y, region.width, region.height
        );
        tracker.add_region(*region);
    }

    // Check merged regions
    println!("\nAfter merging: {} regions", tracker.region_count());
    for (i, region) in tracker.damaged_regions().iter().enumerate() {
        println!(
            "  Region {}: x={}, y={}, {}x{} (area: {})",
            i,
            region.x,
            region.y,
            region.width,
            region.height,
            region.area()
        );
    }

    // Calculate damage statistics
    let frame_size = (1920u32, 1080u32);
    let total_pixels = u64::from(frame_size.0) * u64::from(frame_size.1);
    let damage_ratio = tracker.damage_ratio(frame_size);

    println!("\nDamage statistics:");
    println!(
        "  Frame size: {}x{} ({} pixels)",
        frame_size.0, frame_size.1, total_pixels
    );
    println!("  Total damaged area: {} pixels", tracker.total_damaged_area());
    println!("  Damage ratio: {:.2}%", damage_ratio * 100.0);

    // Encoding decision
    if tracker.should_full_update(frame_size) {
        println!("\nDecision: FULL FRAME UPDATE");
        println!("  Reason: Damage ratio exceeds threshold or too many regions");
    } else {
        println!("\nDecision: PARTIAL UPDATE");
        println!("  Encoding only {} damaged region(s)", tracker.region_count());

        // Get bounding box for single-region optimization
        if let Some(bbox) = tracker.bounding_box() {
            println!(
                "  Bounding box: x={}, y={}, {}x{}",
                bbox.x, bbox.y, bbox.width, bbox.height
            );
        }
    }

    // Simulate another frame with full damage
    tracker.clear();
    tracker.mark_full_damage(frame_size.0, frame_size.1);

    println!("\n--- After full damage frame ---");
    println!("  Should full update: {}", tracker.should_full_update(frame_size));
    println!("  Damage ratio: {:.2}%", tracker.damage_ratio(frame_size) * 100.0);

    // Demonstrate region clipping
    println!("\n--- Region Clipping ---");
    let oversized = DamageRegion::new(1800, 900, 300, 300);
    println!("  Original: x=1800, y=900, 300x300");

    if let Some(clipped) = oversized.clip(frame_size.0, frame_size.1) {
        println!(
            "  Clipped to frame: x={}, y={}, {}x{}",
            clipped.x, clipped.y, clipped.width, clipped.height
        );
    }

    println!("\nExample completed!");
}

#[cfg(not(feature = "damage"))]
fn main() {
    println!("This example requires the 'damage' feature.");
    println!("Run with: cargo run --example with_damage --features damage");
}
