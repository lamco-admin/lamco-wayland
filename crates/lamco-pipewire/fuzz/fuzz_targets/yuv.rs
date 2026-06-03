#![no_main]

use libfuzzer_sys::fuzz_target;
use lamco_pipewire::{PixelFormat, YuvConverter};

// The capture pipeline hands YUV frames to convert_to_bgra: dimensions come from
// the negotiated format, bytes from PipeWire. As a public, Option-returning entry
// point it must never panic or over-read on mismatched dimensions/buffers.
fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let width = u32::from(data[0]) | (u32::from(data[1]) << 8);
    let height = u32::from(data[2]) | (u32::from(data[3]) << 8);
    let format = match data[4] % 3 {
        0 => PixelFormat::NV12,
        1 => PixelFormat::I420,
        _ => PixelFormat::YUY2,
    };
    let mut conv = YuvConverter::new();
    let _ = conv.convert_to_bgra(&data[5..], width, height, format);
});
