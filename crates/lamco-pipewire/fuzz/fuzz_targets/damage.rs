#![no_main]

use libfuzzer_sys::fuzz_target;
use lamco_pipewire::{DamageConfig, DamageDetector};

// detect() diffs successive frames; dimensions come from the negotiated format
// and the buffer from PipeWire. Mismatched sizes must degrade gracefully
// (whole-frame damage) rather than panic or over-read.
fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let width = u32::from(data[0]) | (u32::from(data[1]) << 8);
    let height = u32::from(data[2]) | (u32::from(data[3]) << 8);
    let mut det = DamageDetector::new(DamageConfig::default());
    // Two calls exercise both the baseline frame and the diff path.
    let _ = det.detect(&data[5..], width, height);
    let _ = det.detect(&data[5..], width, height);
});
