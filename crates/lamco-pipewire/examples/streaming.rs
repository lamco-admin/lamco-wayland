//! Adaptive Bitrate Streaming Example
//!
//! This example demonstrates using the adaptive bitrate controller
//! for network-aware streaming of captured content.
//!
//! # Prerequisites
//!
//! - Requires the `adaptive` feature
//!
//! # Running
//!
//! ```bash
//! cargo run --example streaming --features adaptive
//! ```

#[cfg(feature = "adaptive")]
use lamco_pipewire::{
    bitrate::BitrateController,
    config::{AdaptiveBitrateConfig, QualityPreset},
};

#[cfg(feature = "adaptive")]
fn main() {
    println!("lamco-pipewire Adaptive Bitrate Example");
    println!("=======================================");

    // Create a bitrate controller for low-latency streaming
    let config = AdaptiveBitrateConfig::builder()
        .min_bitrate_kbps(500)
        .max_bitrate_kbps(20000)
        .target_fps(60)
        .quality_preset(QualityPreset::LowLatency)
        .calculation_window(30)
        .build();

    println!("\nConfiguration:");
    println!(
        "  Bitrate range: {} - {} kbps",
        config.min_bitrate_kbps, config.max_bitrate_kbps
    );
    println!("  Target FPS: {}", config.target_fps);
    println!("  Quality preset: {:?}", config.quality_preset);
    println!("  Calculation window: {} frames", config.calculation_window);

    let mut controller = BitrateController::new(config);

    println!("\nInitial state:");
    println!("  Recommended bitrate: {} kbps", controller.recommended_bitrate());
    println!("  Recommended quality: {}", controller.recommended_quality());
    println!("  Congestion level: {:.2}", controller.congestion_level());

    // Simulate encoding some frames
    println!("\n--- Simulating good network conditions ---");
    for i in 0..20 {
        // Simulate fast encoding (2ms) with reasonable frame size
        controller.record_frame(2000, 30000 + i * 1000);
    }

    println!("After 20 frames:");
    println!("  Recommended bitrate: {} kbps", controller.recommended_bitrate());
    println!("  Congestion level: {:.2}", controller.congestion_level());

    // Simulate network feedback with some packet loss
    println!("\n--- Simulating packet loss ---");
    controller.record_network_feedback(0.05, 100); // 5% loss, 100ms RTT

    println!("After packet loss:");
    println!("  Recommended bitrate: {} kbps", controller.recommended_bitrate());
    println!("  Recommended quality: {}", controller.recommended_quality());
    println!("  Congestion level: {:.2}", controller.congestion_level());

    // Simulate severe congestion
    println!("\n--- Simulating severe congestion ---");
    controller.record_network_feedback(0.15, 300); // 15% loss, 300ms RTT

    println!("During congestion:");
    println!("  Recommended bitrate: {} kbps", controller.recommended_bitrate());
    println!("  Congestion level: {:.2}", controller.congestion_level());

    // Check frame skipping
    let mut skipped = 0;
    let mut sent = 0;
    for _ in 0..30 {
        if controller.should_skip_frame() {
            skipped += 1;
        } else {
            sent += 1;
        }
    }
    println!("  Frame decisions: {} sent, {} skipped", sent, skipped);

    // Show statistics
    let stats = controller.stats();
    println!("\nStatistics:");
    println!("  Frames recorded: {}", stats.frames_recorded);
    println!("  Frames dropped: {}", stats.frames_dropped);
    println!("  Frames skipped: {}", stats.frames_skipped);
    println!("  Bitrate increases: {}", stats.bitrate_increases);
    println!("  Bitrate decreases: {}", stats.bitrate_decreases);
    println!("  Drop rate: {:.2}%", stats.drop_rate() * 100.0);

    // Demonstrate different presets
    println!("\n--- Quality Presets Comparison ---");

    let presets = [
        ("Low Latency", AdaptiveBitrateConfig::low_latency()),
        ("Balanced", AdaptiveBitrateConfig::default()),
        ("High Quality", AdaptiveBitrateConfig::high_quality()),
    ];

    for (name, preset_config) in &presets {
        let preset_controller = BitrateController::new(preset_config.clone());
        println!(
            "  {}: {} kbps initial, quality {}",
            name,
            preset_controller.recommended_bitrate(),
            preset_controller.recommended_quality()
        );
    }

    // Reset and show recovered state
    println!("\n--- After network recovery ---");
    controller.reset();
    println!("  Recommended bitrate: {} kbps", controller.recommended_bitrate());
    println!("  Congestion level: {:.2}", controller.congestion_level());

    println!("\nExample completed!");
}

#[cfg(not(feature = "adaptive"))]
fn main() {
    println!("This example requires the 'adaptive' feature.");
    println!("Run with: cargo run --example streaming --features adaptive");
}
