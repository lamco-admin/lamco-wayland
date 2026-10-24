//! Runs an audio host on a connection made here to this user's PipeWire, the way
//! a process that can't reach the socket would receive one, and records its
//! sink for a few seconds.
//!
//! ```sh
//! cargo run -p lamco-pipewire --features audio --example audio_host &
//! pw-play --target lamco-example-output some.wav
//! ```

use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lamco_pipewire::{AudioFormat, AudioHostConfig, AudioSamples, CaptureConfig, spawn_audio_host};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| anyhow::anyhow!("XDG_RUNTIME_DIR is unset"))?;
    let socket = PathBuf::from(runtime_dir).join("pipewire-0");
    let connection = OwnedFd::from(UnixStream::connect(&socket)?);

    let host = spawn_audio_host(
        connection,
        AudioHostConfig {
            sink_name: "lamco-example-output".into(),
            sink_description: "Lamco example output".into(),
            ..AudioHostConfig::default()
        },
    )?;
    println!("sink {} is up; play into it now", host.sink_name());

    let mut capture = host.start_capture(
        CaptureConfig {
            sample_rate: 48_000,
            channels: 2,
            format: AudioFormat::I16,
            ..CaptureConfig::default()
        },
        64,
    )?;

    let seconds = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let (mut samples, mut peak) = (0usize, 0i16);
    while Instant::now() < deadline && host.is_alive() {
        match capture.receiver.try_recv() {
            Ok(AudioSamples::I16(chunk)) => {
                samples += chunk.len();
                peak = chunk.iter().fold(peak, |p, s| p.max(s.saturating_abs()));
            }
            Ok(AudioSamples::F32(chunk)) => samples += chunk.len(),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    println!("captured {samples} samples, peak {peak}, host alive {}", host.is_alive());
    Ok(())
}
