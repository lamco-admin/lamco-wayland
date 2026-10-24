//! One PipeWire connection, handed in as a connected socket, carrying a sink
//! and the audio streams that come and go on it.
//!
//! [`spawn_audio_capture`](super::spawn_audio_capture) and
//! [`spawn_virtual_microphone`](super::spawn_virtual_microphone) each open their
//! own connection to the default PipeWire socket, which a process can do when
//! it runs as the user whose PipeWire it wants. A process that can't reach
//! that socket, because it runs as another user, gets a connection someone
//! else made instead, once, as a file descriptor. It can't open more, so
//! everything it does with PipeWire has to share that one connection, for as
//! long as the process lives. [`AudioHost`] is that: one thread owning the
//! connection, a sink created on it for applications to play into, and the
//! capture and microphone streams the owner starts and stops on it.

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc as std_mpsc};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use pipewire as pw;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::{
    AudioCaptureHandle, AudioSamples, CaptureConfig, PlaybackConfig, VirtualMicrophoneHandle, connect_capture_stream,
    connect_microphone_stream,
};

/// How often the host thread looks for new commands between PipeWire events.
const COMMAND_POLL: Duration = Duration::from_millis(20);

/// The sink an [`AudioHost`] creates.
#[derive(Debug, Clone)]
pub struct AudioHostConfig {
    /// `node.name` of the sink, which captures target.
    pub sink_name: String,
    /// `node.description`, the name a sound settings panel shows.
    pub sink_description: String,
    /// `priority.session`. Higher than any device's makes the session manager
    /// choose this sink as the default, so applications play into it unasked.
    pub sink_priority: u32,
}

impl Default for AudioHostConfig {
    fn default() -> Self {
        Self {
            sink_name: "lamco-output".into(),
            sink_description: "Lamco Remote Audio".into(),
            sink_priority: 2000,
        }
    }
}

enum Command {
    Capture {
        config: CaptureConfig,
        sender: mpsc::Sender<AudioSamples>,
        stop_signal: Arc<AtomicBool>,
    },
    Microphone {
        config: PlaybackConfig,
        node_label: Option<String>,
        receiver: mpsc::Receiver<AudioSamples>,
        stop_signal: Arc<AtomicBool>,
    },
}

/// A PipeWire connection with a sink on it, run on its own thread.
///
/// The sink exists from [`spawn_audio_host`] until the host is dropped, the
/// connection fails, or [`AudioHost::stop`] is called. Captures and
/// microphones started on it end when their handles say stop, or with the
/// host.
pub struct AudioHost {
    commands: std_mpsc::Sender<Command>,
    stop_signal: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    sink_name: String,
}

impl AudioHost {
    /// Records the host's sink, as [`spawn_audio_capture`](super::spawn_audio_capture)
    /// records the default sink.
    pub fn start_capture(&self, config: CaptureConfig, channel_size: usize) -> Result<AudioCaptureHandle> {
        let (sender, receiver) = mpsc::channel(channel_size);
        let stop_signal = Arc::new(AtomicBool::new(false));
        self.send(Command::Capture {
            config,
            sender,
            stop_signal: Arc::clone(&stop_signal),
        })?;
        Ok(AudioCaptureHandle { receiver, stop_signal })
    }

    /// Creates a virtual microphone on the host's connection, as
    /// [`spawn_virtual_microphone`](super::spawn_virtual_microphone) does on its own.
    pub fn start_microphone(
        &self,
        config: PlaybackConfig,
        node_label: Option<String>,
        channel_size: usize,
    ) -> Result<VirtualMicrophoneHandle> {
        let (sender, receiver) = mpsc::channel(channel_size);
        let stop_signal = Arc::new(AtomicBool::new(false));
        self.send(Command::Microphone {
            config,
            node_label,
            receiver,
            stop_signal: Arc::clone(&stop_signal),
        })?;
        Ok(VirtualMicrophoneHandle { sender, stop_signal })
    }

    /// Whether the host's thread is still running with its connection.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// `node.name` of the host's sink.
    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }

    /// Ends the host: its streams, its sink and its connection.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }

    fn send(&self, command: Command) -> Result<()> {
        if !self.is_alive() {
            return Err(anyhow!("the audio host has ended"));
        }
        self.commands
            .send(command)
            .map_err(|_| anyhow!("the audio host has ended"))
    }
}

impl Drop for AudioHost {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Runs an [`AudioHost`] on `connection`, a socket already connected to a
/// PipeWire daemon.
///
/// PipeWire took the connecting process's credentials when the socket
/// connected, so the host acts with that user's PipeWire permissions, whoever
/// runs it. The sink is created before this returns its thread's result: an
/// error here means the connection or the sink failed.
pub fn spawn_audio_host(connection: OwnedFd, config: AudioHostConfig) -> Result<AudioHost> {
    let (commands, command_rx) = std_mpsc::channel();
    let stop_signal = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
    let sink_name = config.sink_name.clone();

    let thread_stop = Arc::clone(&stop_signal);
    let thread_alive = Arc::clone(&alive);
    std::thread::Builder::new()
        .name("pipewire-host".into())
        .spawn(move || {
            crate::pw_lifecycle::acquire();
            if let Err(e) = run_host(connection, &config, &command_rx, &thread_stop, &ready_tx) {
                error!("Audio host error: {:#}", e);
                let _ = ready_tx.try_send(Err(format!("{e:#}")));
            }
            thread_alive.store(false, Ordering::SeqCst);
            crate::pw_lifecycle::release();
        })
        .context("Failed to spawn audio host thread")?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(AudioHost {
            commands,
            stop_signal,
            alive,
            sink_name,
        }),
        Ok(Err(message)) => Err(anyhow!(message)),
        Err(_) => Err(anyhow!("the audio host thread ended before it was ready")),
    }
}

fn run_host(
    connection: OwnedFd,
    config: &AudioHostConfig,
    commands: &std_mpsc::Receiver<Command>,
    stop_signal: &Arc<AtomicBool>,
    ready: &std_mpsc::SyncSender<std::result::Result<(), String>>,
) -> Result<()> {
    let mainloop = pw::main_loop::MainLoopBox::new(None).context("Failed to create PipeWire MainLoop")?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None).context("Failed to create PipeWire Context")?;
    let core = context
        .connect_fd(connection, None)
        .context("Failed to use the PipeWire connection")?;

    // The daemon going away, or closing the socket, arrives as an error on the
    // core object.
    let connection_failed = Arc::new(AtomicBool::new(false));
    let failed = Arc::clone(&connection_failed);
    let _core_listener = core
        .add_listener_local()
        .error(move |id, _seq, res, message| {
            if id == pw::core::PW_ID_CORE {
                warn!(res, message, "PipeWire connection failed");
                failed.store(true, Ordering::SeqCst);
            } else {
                debug!(id, res, message, "PipeWire object error");
            }
        })
        .register();

    let priority = config.sink_priority.to_string();
    let sink_props = pw::properties::properties! {
        "factory.name" => "support.null-audio-sink",
        *pw::keys::NODE_NAME => config.sink_name.as_str(),
        *pw::keys::NODE_DESCRIPTION => config.sink_description.as_str(),
        *pw::keys::MEDIA_CLASS => "Audio/Sink",
        "audio.position" => "FL,FR",
        "priority.session" => priority.as_str(),
        "monitor.channel-volumes" => "true",
        // Gone with the connection, so a sink never outlives its owner.
        "object.linger" => "false",
    };
    let _sink: pw::node::Node = core
        .create_object("adapter", &sink_props)
        .context("Failed to create the sink")?;
    info!(sink = config.sink_name, "Audio host sink created");
    let _ = ready.try_send(Ok(()));

    let mut captures = Vec::new();
    let mut microphones = Vec::new();
    let loop_ref = mainloop.loop_();
    while !stop_signal.load(Ordering::Relaxed) && !connection_failed.load(Ordering::Relaxed) {
        loop_ref.iterate(pw::loop_::Timeout::Finite(COMMAND_POLL));

        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Capture {
                    config: capture_config,
                    sender,
                    stop_signal,
                } => match connect_capture_stream(
                    &core,
                    &capture_config,
                    None,
                    Some(&config.sink_name),
                    sender,
                    &stop_signal,
                ) {
                    Ok(stream) => captures.push((stream, stop_signal)),
                    Err(e) => {
                        warn!("Audio host capture failed: {:#}", e);
                        stop_signal.store(true, Ordering::SeqCst);
                    }
                },
                Command::Microphone {
                    config: playback_config,
                    node_label,
                    receiver,
                    stop_signal,
                } => match connect_microphone_stream(
                    &core,
                    &playback_config,
                    node_label.as_deref(),
                    receiver,
                    &stop_signal,
                ) {
                    Ok(stream) => microphones.push((stream, stop_signal)),
                    Err(e) => {
                        warn!("Audio host microphone failed: {:#}", e);
                        stop_signal.store(true, Ordering::SeqCst);
                    }
                },
            }
        }

        // A stopped stream disconnects when it's dropped.
        captures.retain(|(_, stop)| !stop.load(Ordering::Relaxed));
        microphones.retain(|(_, stop)| !stop.load(Ordering::Relaxed));
    }

    // The streams still running end with the host; their handles say so.
    for stop in captures
        .iter()
        .map(|(_, stop)| stop)
        .chain(microphones.iter().map(|(_, stop)| stop))
    {
        stop.store(true, Ordering::SeqCst);
    }
    if connection_failed.load(Ordering::Relaxed) {
        return Err(anyhow!("the PipeWire connection failed"));
    }
    info!("Audio host stopped");
    Ok(())
}
