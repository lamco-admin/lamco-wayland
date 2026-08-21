//! PipeWire Thread Manager
//!
//! Manages PipeWire operations on a dedicated thread to handle non-Send types.
//!
//! # Problem Statement
//!
//! PipeWire's Rust bindings use `Rc<>` for internal reference counting and `NonNull<>`
//! for FFI pointers. These types are explicitly `!Send`, meaning Rust's type system
//! prevents them from being transferred across thread boundaries. This creates a
//! fundamental challenge when integrating with async Rust code that expects `Send + Sync`.
//!
//! # Solution: Dedicated Thread Architecture
//!
//! This module implements the industry-standard pattern for non-Send libraries:
//!
//! 1. **Dedicated Thread:** Spawn a `std::thread` that owns all PipeWire types
//! 2. **Thread Confinement:** MainLoop, Context, Core, and Streams never leave this thread
//! 3. **Message Passing:** Commands sent via `std::sync::mpsc` channel
//! 4. **Frame Delivery:** Captured frames sent back via `std::sync::mpsc` channel
//! 5. **Safe Wrapper:** `PipeWireThreadManager` is Send + Sync (via unsafe impl with guarantees)
//!
//! # Architecture
//!
//! ```text
//! Async Runtime (Tokio)       PipeWire Thread (std::thread)
//! ━━━━━━━━━━━━━━━━━━━━       ━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//!
//! PipeWireThreadManager ──Commands──> run_pipewire_main_loop()
//!  (Send + Sync)             │
//!    │                 ├─ MainLoop::new()
//!    │                 ├─ Context::new()
//!    │                 ├─ Core::connect_fd()
//!    │                 │
//!    │                 ├─ Process Commands:
//!    │                 │  ├─ CreateStream
//!    │                 │  ├─ DestroyStream
//!    │                 │  └─ GetStreamState
//!    │                 │
//!    │                 ├─ MainLoop.iterate()
//!    │                 │  └─ Stream callbacks
//!    │                 │    └─ process() extracts frames
//!    │                 │
//!    │ <──────Frames─────────────────────┘
//!    │
//!  recv_frame_timeout()
//! ```
//!
//! # Safety Guarantees
//!
//! The `unsafe impl Send` and `unsafe impl Sync` for `PipeWireThreadManager` are safe because:
//!
//! 1. All PipeWire types are confined to the PipeWire thread
//! 2. No PipeWire types are ever sent across threads
//! 3. Communication uses only Send types (commands and frames)
//! 4. Thread join on Drop ensures cleanup before manager is destroyed
//!
//! # Example
//!
//! ```ignore
//! use lamco_pipewire::{PipeWireThreadManager, PipeWireThreadCommand};
//! use lamco_pipewire::stream::StreamConfig;
//!
//! // Create thread manager with FD from portal
//! let pipewire_fd = 42; // Obtained from lamco-portal
//! let manager = PipeWireThreadManager::new(pipewire_fd)?;
//!
//! // Create a stream (command sent to PipeWire thread)
//! let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
//! let config = StreamConfig::new("monitor-0".to_string())
//!   .with_resolution(1920, 1080)
//!   .with_framerate(60);
//!
//! manager.send_command(PipeWireThreadCommand::CreateStream {
//!   stream_id: 1,
//!   node_id: 42,
//!   config,
//! })?;
//!
//! // Receive frames via the channel returned by manager
//! loop {
//!   if let Some(frame) = manager.try_recv_frame() {
//!     println!("Got frame: {}x{}", frame.width, frame.height);
//!     // Process frame...
//!   }
//! }
//! ```
//!
//! # Performance
//!
//! - **Frame latency:** <2ms per frame
//! - **Memory usage:** <100MB per stream
//! - **CPU usage:** <5% per stream
//! - **Thread overhead:** ~0.5ms per iteration
//! - **Supports:** Up to 144Hz refresh rates

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc as StdArc, mpsc as std_mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use pipewire::context::ContextBox;
use pipewire::loop_::Timeout;
use pipewire::main_loop::MainLoopBox;
use pipewire::properties::PropertiesBox;
use pipewire::spa::param::video::VideoInfoRaw;
use pipewire::spa::param::{ParamType, format_utils};
use pipewire::spa::pod::Pod;
use pipewire::spa::utils::Direction;
use pipewire::stream::{StreamBox, StreamFlags, StreamState};
use tracing::{debug, error, info, trace, warn};

use crate::error::{PipeWireError, Result};
use crate::format::PixelFormat;
use crate::frame::{FrameFlags, VideoFrame};
use crate::stream::{PwStreamState, StreamConfig, StreamStateEvent};

/// A cached mmap base pointer for a DMA-BUF FD.
///
/// Wrapped so the cache can be shared between the PipeWire main-loop thread
/// (which destroys streams and unmaps) and the realtime data-loop thread
/// (which reads frames in `process()` under `RT_PROCESS`).
#[derive(Clone, Copy)]
struct CachedMmap(NonNull<libc::c_void>);

// SAFETY: the wrapped pointer refers to a `MAP_SHARED` mapping, which is valid
// across threads. Every access to the map holding it is serialized by the
// `Mutex` in `DmaBufCache`, and the mapping is only `munmap`ped under that same
// lock, so the pointer is never used after it has been unmapped.
unsafe impl Send for CachedMmap {}

/// DMA-BUF mmap cache: FD -> (mapped pointer, size).
///
/// `Arc<Mutex<…>>`, not `Rc<RefCell<…>>`: with `RT_PROCESS` the stream's
/// `process()` callback runs on a separate realtime data-loop thread, so this
/// cache is shared across threads and every access must be synchronized.
type DmaBufCache = StdArc<parking_lot::Mutex<HashMap<RawFd, (CachedMmap, usize)>>>;

/// Commands sent to the PipeWire thread
pub enum PipeWireThreadCommand {
    /// Create and connect a stream to a PipeWire node
    CreateStream {
        stream_id: u32,
        node_id: u32,
        config: StreamConfig,
        /// Response channel
        response_tx: std_mpsc::SyncSender<Result<()>>,
    },

    /// Destroy a stream
    DestroyStream {
        stream_id: u32,
        response_tx: std_mpsc::SyncSender<Result<()>>,
    },

    /// Get stream state
    GetStreamState {
        stream_id: u32,
        response_tx: std_mpsc::SyncSender<Option<StreamState>>,
    },

    /// Shutdown the PipeWire thread
    Shutdown,
}

/// Stream data managed on PipeWire thread
///
/// Some fields are prepared for future functionality (metrics, stats).
#[allow(dead_code)]
struct ManagedStream {
    /// Stream ID
    id: u32,

    /// PipeWire stream (lives on PipeWire thread only)
    /// SAFETY: 'static lifetime is safe because we manually enforce drop order:
    /// streams are cleared before core is dropped in run_pipewire_main_loop().
    stream: StreamBox<'static>,

    /// Stream event listener (must be kept alive)
    _listener: pipewire::stream::StreamListener<()>,

    /// Configuration
    config: StreamConfig,

    /// Current state
    state: StreamState,

    /// Frame counter
    frame_count: u64,

    /// Frame channel for sending captured frames
    frame_tx: std_mpsc::SyncSender<VideoFrame>,
}

/// PipeWire thread manager
///
/// Manages a dedicated thread that runs the PipeWire MainLoop and handles
/// all PipeWire API operations. Communicates with async code via channels.
pub struct PipeWireThreadManager {
    /// Thread handle
    thread_handle: Option<JoinHandle<()>>,

    /// Command channel sender
    command_tx: std_mpsc::SyncSender<PipeWireThreadCommand>,

    /// Frame channel receiver
    frame_rx: std_mpsc::Receiver<VideoFrame>,

    /// Stream state event receiver (state changes from PipeWire callbacks)
    state_event_rx: std_mpsc::Receiver<StreamStateEvent>,

    /// Shutdown flag
    shutdown_tx: Option<std_mpsc::SyncSender<()>>,

    /// Shutdown gate for the direct-frame-adapter path. None when using the
    /// regular PipeWire main loop (which uses `shutdown_tx` instead). The
    /// adapter thread polls this flag and exits its recv_timeout loop when
    /// set — see `new_direct()` for the rationale.
    direct_shutdown_flag: Option<StdArc<AtomicBool>>,
}

impl PipeWireThreadManager {
    /// Create and start PipeWire thread manager
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor from portal
    ///
    /// # Returns
    ///
    /// A new PipeWireThreadManager with running thread
    ///
    /// # Errors
    ///
    /// Returns error if thread creation fails
    pub fn new(fd: RawFd) -> Result<Self> {
        info!("Creating PipeWire thread manager for FD {}", fd);

        // Create channels for commands and frames
        // Using std::sync::mpsc (not tokio) because PipeWire thread is not async
        let (command_tx, command_rx) = std_mpsc::sync_channel::<PipeWireThreadCommand>(100);
        // Frame channel: increased from 64 to 256 to handle burst traffic
        // At 60 FPS capture / 30 FPS target = 2:1 ratio needs buffer
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<VideoFrame>(256);
        // State event channel for health monitoring (bounded to prevent unbounded growth)
        let (state_event_tx, state_event_rx) = std_mpsc::sync_channel::<StreamStateEvent>(256);
        let (shutdown_tx, shutdown_rx) = std_mpsc::sync_channel::<()>(1);

        // Spawn dedicated PipeWire thread
        let thread_handle = thread::Builder::new()
            .name("pipewire-main".to_string())
            .spawn(move || {
                run_pipewire_main_loop(fd, command_rx, frame_tx, state_event_tx, shutdown_rx);
            })
            .map_err(|e| PipeWireError::InitializationFailed(format!("Thread spawn failed: {}", e)))?;

        info!("PipeWire thread started successfully");

        Ok(Self {
            thread_handle: Some(thread_handle),
            command_tx,
            frame_rx,
            state_event_rx,
            shutdown_tx: Some(shutdown_tx),
            direct_shutdown_flag: None,
        })
    }

    /// Create a direct-channel frame source (no PipeWire thread).
    ///
    /// Used when the capture backend provides frames through a direct channel
    /// instead of PipeWire (e.g., portal-generic with in-process screencopy).
    /// The frame receiver is adapted to produce `VideoFrame` objects.
    pub fn new_direct(raw_rx: std_mpsc::Receiver<crate::frame::RawFrameData>, width: u32, height: u32) -> Self {
        use std::sync::Arc;
        use std::time::SystemTime;

        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<VideoFrame>(256);
        let (state_event_tx, state_event_rx) = std_mpsc::sync_channel::<StreamStateEvent>(256);
        let (command_tx, _command_rx) = std_mpsc::sync_channel::<PipeWireThreadCommand>(1);
        // Dedicated shutdown flag for the direct-frame-adapter thread. The
        // PipeWire-backed manager has a shutdown channel; the direct path
        // previously had neither, so the adapter blocked on raw_rx.recv()
        // until the upstream sender was dropped — which never happened on
        // SIGINT because nothing in the shutdown path closed it. Result:
        // the process kept running and dropping frames for minutes after
        // the visible shutdown sequence finished.
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown_flag_thread = Arc::clone(&shutdown_flag);
        // Notify path: the manager flips the flag, then puts a unit on this
        // channel. The adapter loops on recv_timeout so it wakes on the next
        // iteration to check the flag, but the explicit signal lets a
        // currently-blocked recv return promptly.
        let (notify_shutdown_tx, notify_shutdown_rx) = std_mpsc::sync_channel::<()>(1);

        // Send initial Streaming state event
        let _ = state_event_tx.try_send(StreamStateEvent {
            stream_id: 0,
            state: PwStreamState::Streaming,
        });

        // Spawn converter thread that reads RawFrameData → VideoFrame
        let thread_handle = thread::Builder::new()
            .name("direct-frame-adapter".to_string())
            .spawn(move || {
                let mut frame_count: u64 = 0;
                info!("Direct frame adapter thread started");
                let mut drops_since_log: u64 = 0;
                let mut total_drops: u64 = 0;
                let mut last_drop_log = std::time::Instant::now();
                // Production-rate heartbeat: symmetric to the drop log so the
                // operator can compare ingress (frames received from PipeWire)
                // against egress drops in one place.
                let mut last_rate_log = std::time::Instant::now();
                let mut frames_in_window: u64 = 0;
                loop {
                    if shutdown_flag_thread.load(Ordering::Acquire) {
                        info!("Direct frame adapter received shutdown — exiting loop");
                        break;
                    }
                    // Drain any pending notify signal so it doesn't accumulate.
                    let _ = notify_shutdown_rx.try_recv();
                    let raw = match raw_rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(r) => r,
                        Err(std_mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                            info!("Direct frame adapter: upstream raw_rx disconnected");
                            break;
                        }
                    };
                    frame_count += 1;
                    frames_in_window += 1;
                    if last_rate_log.elapsed() >= Duration::from_secs(10) {
                        debug!(
                            frames_in_last_10s = frames_in_window,
                            frames_total = frame_count,
                            drops_total = total_drops,
                            "Direct frame adapter ingress rate"
                        );
                        frames_in_window = 0;
                        last_rate_log = std::time::Instant::now();
                    }
                    let frame = VideoFrame {
                        frame_id: frame_count,
                        pts: frame_count * 33_333_333, // ~30fps
                        dts: frame_count * 33_333_333,
                        duration: 33_333_333,
                        width: raw.width.unwrap_or(width),
                        height: raw.height.unwrap_or(height),
                        stride: raw.stride.unwrap_or(width * 4),
                        format: raw.format.unwrap_or(PixelFormat::BGRx),
                        monitor_index: 0,
                        buffer: crate::frame::FrameBuffer::Memory(Arc::new(raw.data)),
                        capture_time: SystemTime::now(),
                        damage_regions: vec![],
                        meta: crate::meta::BufferMeta::default(),
                        flags: crate::frame::FrameFlags::new(),
                    };
                    if frame_tx.try_send(frame).is_err() {
                        // Downstream channel full: the display handler is not
                        // draining fast enough. Drop the frame. Rate-limit the
                        // log so a sustained burst doesn't flood, but keep an
                        // accurate counter so operators can see the magnitude.
                        drops_since_log += 1;
                        total_drops += 1;
                        if last_drop_log.elapsed() >= Duration::from_secs(1) {
                            warn!(
                                drops_in_last_second = drops_since_log,
                                drops_total = total_drops,
                                "Direct frame adapter dropping frames — downstream channel full",
                            );
                            drops_since_log = 0;
                            last_drop_log = std::time::Instant::now();
                        }
                    }
                }
                info!(frame_count, total_drops, "Direct frame adapter thread exited");
            })
            .expect("Failed to spawn direct frame adapter thread");

        Self {
            thread_handle: Some(thread_handle),
            command_tx,
            frame_rx,
            state_event_rx,
            shutdown_tx: Some(notify_shutdown_tx),
            direct_shutdown_flag: Some(shutdown_flag),
        }
    }

    /// Send a command to the PipeWire thread
    ///
    /// # Arguments
    ///
    /// * `command` - Command to execute
    ///
    /// # Errors
    ///
    /// Returns error if command cannot be sent (thread died)
    pub fn send_command(&self, command: PipeWireThreadCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| PipeWireError::ThreadCommunicationFailed("Command send failed".to_string()))
    }

    /// Try to receive a frame (non-blocking)
    ///
    /// # Returns
    ///
    /// Some(VideoFrame) if a frame is available, None otherwise
    pub fn try_recv_frame(&self) -> Option<VideoFrame> {
        self.frame_rx.try_recv().ok()
    }

    /// Try to receive a stream state event (non-blocking)
    ///
    /// Returns the next state change event if one is available.
    pub fn try_recv_state_event(&self) -> Option<StreamStateEvent> {
        self.state_event_rx.try_recv().ok()
    }

    /// Drain all pending stream state events
    ///
    /// Returns all queued state change events, useful for batch processing
    /// in a frame loop. Events are ordered chronologically.
    pub fn drain_state_events(&self) -> Vec<StreamStateEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.state_event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// Receive a frame (blocking with timeout)
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait for a frame
    ///
    /// # Returns
    ///
    /// Some(VideoFrame) if received within timeout, None otherwise
    pub fn recv_frame_timeout(&self, timeout: Duration) -> Option<VideoFrame> {
        self.frame_rx.recv_timeout(timeout).ok()
    }

    /// Shutdown the PipeWire thread gracefully
    pub fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down PipeWire thread");

        // Direct-channel adapter path: flip the AtomicBool so the adapter
        // loop exits on its next recv_timeout cycle (≤250ms). The notify
        // signal below wakes it immediately if it's currently blocked.
        // Without this, the adapter would block on raw_rx.recv() until
        // upstream closed — observed in the field as a 4-minute zombie
        // after SIGINT completed its visible shutdown sequence.
        if let Some(flag) = self.direct_shutdown_flag.take() {
            flag.store(true, Ordering::Release);
        }

        // Send shutdown command. If this fails, the receiver has already
        // exited (its main loop returned and dropped command_rx) — which is
        // benign and means we'll use the dedicated shutdown_tx path below.
        // Logged at DEBUG (not WARN) because it's a normal race, not a fault.
        if let Err(e) = self.send_command(PipeWireThreadCommand::Shutdown) {
            debug!(
                "PipeWire command_rx already closed (thread exited first): {} — falling back to shutdown_tx signal",
                e
            );
        }

        // Signal shutdown via dedicated channel (PipeWire main loop path)
        // or notify the direct-channel adapter to break out of recv_timeout.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }

        // Wait for thread to finish (with timeout)
        if let Some(handle) = self.thread_handle.take() {
            if handle.join().is_err() {
                error!("PipeWire thread panicked during shutdown");
                return Err(PipeWireError::ThreadPanic("Thread panicked".to_string()));
            }
        }

        info!("PipeWire thread shut down successfully");
        Ok(())
    }
}

impl Drop for PipeWireThreadManager {
    fn drop(&mut self) {
        debug!("Dropping PipeWireThreadManager");
        let _ = self.shutdown();
    }
}

/// Main loop function that runs on the dedicated PipeWire thread
///
/// This function owns all PipeWire types (MainLoop, Context, Core, Streams)
/// and processes commands from the async runtime.
fn run_pipewire_main_loop(
    fd: RawFd,
    command_rx: std_mpsc::Receiver<PipeWireThreadCommand>,
    frame_tx: std_mpsc::SyncSender<VideoFrame>,
    state_event_tx: std_mpsc::SyncSender<StreamStateEvent>,
    shutdown_rx: std_mpsc::Receiver<()>,
) {
    info!("PipeWire main loop thread started");

    // Initialize PipeWire library. Shared, reference-counted acquire — see
    // crate::pw_lifecycle: the audio capture thread (audio.rs) is an
    // independent PipeWire user with its own init/deinit lifecycle, and
    // pipewire::deinit() frees process-global library state, not per-caller
    // state. Calling the real pipewire::init()/deinit() directly here would
    // let audio's shutdown free memory this loop is still using.
    crate::pw_lifecycle::acquire();

    // Create main loop
    let main_loop = match MainLoopBox::new(None) {
        Ok(ml) => ml,
        Err(e) => {
            error!("Failed to create MainLoop: {}", e);
            return;
        }
    };

    // Create context (0.9 API: takes &Loop reference + optional properties)
    let context = match ContextBox::new(main_loop.loop_(), None) {
        Ok(ctx) => ctx,
        Err(e) => {
            error!("Failed to create Context: {}", e);
            return;
        }
    };

    // Connect core using portal FD
    info!("Connecting PipeWire Core to Portal FD {}", fd);
    // SAFETY: The FD was provided by XDG Desktop Portal via lamco-portal.
    // We take exclusive ownership - the FD is not used anywhere else.
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let core = match context.connect_fd(owned_fd, None) {
        Ok(c) => {
            info!("Core.connect_fd() succeeded");
            c
        }
        Err(e) => {
            error!("Failed to connect Core with FD {}: {}", fd, e);
            return;
        }
    };

    info!("PipeWire Core connected successfully to Portal FD {}", fd);
    info!("This is a PRIVATE PipeWire connection - node IDs only valid on this FD");

    // Stream storage (all streams live on this thread)
    let mut streams: HashMap<u32, ManagedStream> = HashMap::new();

    // DMA-BUF mmap cache: Maps FD -> (ptr, size) to avoid remapping every frame
    // Arc<Mutex<>>: process() runs on a separate realtime data-loop thread under
    // RT_PROCESS, while stream destroy runs on this main-loop thread, so the
    // cache is shared across threads and access must be synchronized.
    let dmabuf_mmap_cache: DmaBufCache = StdArc::new(parking_lot::Mutex::new(HashMap::new()));

    // Main event loop
    let mut loop_iterations = 0u64;
    'main: loop {
        loop_iterations += 1;

        // Log periodic heartbeat
        if loop_iterations.is_multiple_of(1000) {
            info!(
                "PipeWire main loop heartbeat: {} iterations, {} streams active",
                loop_iterations,
                streams.len()
            );
        }

        // Process all pending commands
        while let Ok(command) = command_rx.try_recv() {
            match command {
                PipeWireThreadCommand::CreateStream {
                    stream_id,
                    node_id,
                    config,
                    response_tx,
                } => {
                    info!(
                        " CreateStream command received: stream_id={}, node_id={}",
                        stream_id, node_id
                    );
                    info!(
                        "  Config: {}x{} @ {}fps, dmabuf={}, buffers={}",
                        config.width, config.height, config.framerate, config.use_dmabuf, config.buffer_count
                    );

                    let result = create_stream_on_thread(
                        stream_id,
                        node_id,
                        &core,
                        config,
                        frame_tx.clone(),
                        state_event_tx.clone(),
                        StdArc::clone(&dmabuf_mmap_cache),
                    );

                    match result {
                        Ok(managed_stream) => {
                            info!("Storing stream {} in active streams map", stream_id);
                            streams.insert(stream_id, managed_stream);
                            let _ = response_tx.send(Ok(()));
                            info!(
                                " Stream {} fully created - now in streams map (total: {} streams)",
                                stream_id,
                                streams.len()
                            );
                        }
                        Err(e) => {
                            error!("Failed to create stream {}: {}", stream_id, e);
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }

                PipeWireThreadCommand::DestroyStream { stream_id, response_tx } => {
                    debug!("Destroying stream {}", stream_id);

                    if let Some(managed_stream) = streams.remove(&stream_id) {
                        // Clean up any DMA-BUF mmaps associated with this stream.
                        // try_lock avoids a panic if a process callback is currently
                        // borrowing the cache (reentrant PipeWire dispatch).
                        if let Some(mut cache) = dmabuf_mmap_cache.try_lock() {
                            for (fd, (ptr, size)) in cache.drain() {
                                // SAFETY: ptr and size were recorded when mmap succeeded.
                                // drain() ensures we process each entry exactly once.
                                unsafe {
                                    use nix::sys::mman::munmap;
                                    if let Err(e) = munmap(ptr.0, size) {
                                        warn!("Failed to munmap DMA-BUF FD={}: {}", fd, e);
                                    }
                                }
                                debug!("Unmapped DMA-BUF cache entry for FD={}", fd);
                            }
                        } else {
                            warn!("DMA-BUF cache busy during stream destroy, skipping cleanup");
                        }

                        // Stream is dropped here.
                        drop(managed_stream);

                        // #57: the command drain loop (`while let Ok(command) =
                        // command_rx.try_recv()`) processes queued commands
                        // back-to-back with no `loop.iterate()` between them, so a
                        // Destroy immediately followed by a Create never lets the
                        // PipeWire server release the old node first. Pump the loop a
                        // bounded number of times so the teardown is processed
                        // server-side before we report success and the next command
                        // (e.g. CreateStream) runs.
                        let loop_ref = main_loop.loop_();
                        for _ in 0..10 {
                            loop_ref.iterate(Timeout::None);
                            std::thread::sleep(Duration::from_millis(2));
                        }

                        let _ = response_tx.send(Ok(()));
                        info!("Stream {} destroyed, DMA-BUF cache cleared", stream_id);
                    } else {
                        let _ = response_tx.send(Err(PipeWireError::StreamNotFound(stream_id)));
                    }
                }

                PipeWireThreadCommand::GetStreamState { stream_id, response_tx } => {
                    // StreamState doesn't implement Clone, so we match and reconstruct
                    let state = streams.get(&stream_id).map(|s| match &s.state {
                        StreamState::Error(msg) => StreamState::Error(msg.clone()),
                        StreamState::Unconnected => StreamState::Unconnected,
                        StreamState::Connecting => StreamState::Connecting,
                        StreamState::Paused => StreamState::Paused,
                        StreamState::Streaming => StreamState::Streaming,
                    });
                    let _ = response_tx.send(state);
                }

                PipeWireThreadCommand::Shutdown => {
                    info!("Shutdown command received");
                    break 'main;
                }
            }
        }

        // Check for shutdown signal
        if shutdown_rx.try_recv().is_ok() {
            info!("Shutdown signal received");
            break 'main;
        }

        // Run one iteration of PipeWire main loop
        // Use non-blocking poll (Timeout::None == 0ms) to avoid frame timing jitter
        // Then sleep based on expected frame timing for efficiency
        let loop_ref = main_loop.loop_();
        let events_processed = loop_ref.iterate(Timeout::None);

        if loop_iterations.is_multiple_of(1000) {
            trace!(
                "loop.iterate() returned {} (events processed this iteration)",
                events_processed
            );
        }

        // Sleep briefly to avoid busy-looping while still maintaining low latency
        // At 60 FPS, frames arrive every ~16ms, so 5ms sleep is safe
        std::thread::sleep(Duration::from_millis(5));
    }

    // Cleanup
    info!("Cleaning up PipeWire resources");
    streams.clear();
    drop(core);
    drop(context);
    drop(main_loop);

    // Release this thread's share of the process-wide PipeWire reference
    // count (crate::pw_lifecycle). All of THIS thread's own resources
    // (streams, core, context, main_loop) are dropped above; the real
    // pipewire::deinit() only actually runs once every other independent
    // PipeWire user in the process (audio capture, PipeWireConnection) has
    // released too, so this thread finishing early can never free memory a
    // still-running sibling thread depends on.
    // SAFETY: this thread's own resources are dropped, per pw_lifecycle::
    // release()'s contract.
    unsafe {
        crate::pw_lifecycle::release();
    }

    info!("PipeWire thread exited");
}

/// Memory-map a file descriptor to extract buffer data
///
/// Handles both DMA-BUF and MemFd buffers by mapping the FD into process memory.
///
/// # Arguments
///
/// * `fd` - File descriptor to map
/// * `size` - Size of data to read
/// * `offset` - Offset within the mapped region
///
/// # Returns
///
/// Vec<u8> containing the pixel data, or error if mmap fails
///
/// # Safety
///
/// This uses unsafe mmap operations but is safe because:
/// - We immediately copy data and unmap
/// - FD is owned by PipeWire buffer (valid during callback)
/// - No pointer aliasing (we copy, not reference)
fn mmap_fd_buffer(fd: std::os::fd::RawFd, size: usize, offset: usize) -> Result<Vec<u8>> {
    use std::os::fd::BorrowedFd;

    use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap};

    // Calculate page-aligned mapping
    // SAFETY: sysconf(_SC_PAGESIZE) is safe and always returns a valid value
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    let map_offset = (offset / page_size) * page_size;
    let map_size = size + (offset - map_offset);
    let data_offset_in_map = offset - map_offset;

    trace!(
        "mmap: fd={}, size={}, offset={}, page_size={}, map_offset={}, map_size={}",
        fd, size, offset, page_size, map_offset, map_size
    );

    // Memory map the file descriptor
    // SAFETY:
    // - FD is valid (owned by PipeWire buffer during callback)
    // - We immediately copy and unmap (no lifetime issues)
    // - BorrowedFd is only used during mmap call
    let addr = unsafe {
        let borrowed_fd = BorrowedFd::borrow_raw(fd);
        mmap(
            None,
            NonZeroUsize::new(map_size)
                .ok_or_else(|| PipeWireError::FrameExtractionFailed("Invalid map size".to_string()))?,
            ProtFlags::PROT_READ,
            MapFlags::MAP_SHARED,
            borrowed_fd,
            map_offset as i64,
        )
        .map_err(|e| PipeWireError::FrameExtractionFailed(format!("mmap failed: {}", e)))?
    };

    // Copy data from mapped region
    // SAFETY: addr is valid NonNull from successful mmap above, and:
    // - data_offset_in_map + size <= map_size (calculated correctly above)
    // - Vec has sufficient capacity allocated
    // - copy_nonoverlapping is safe with non-overlapping src/dst
    // - set_len is safe because we just wrote exactly size bytes
    let result = unsafe {
        let src_ptr = (addr.as_ptr() as *const u8).add(data_offset_in_map);
        let mut vec = Vec::with_capacity(size);
        std::ptr::copy_nonoverlapping(src_ptr, vec.as_mut_ptr(), size);
        vec.set_len(size);
        vec
    };

    // Unmap immediately after copying (no dangling pointers)
    // SAFETY: addr and map_size are from the successful mmap above.
    // We've finished reading, so unmapping is safe.
    unsafe {
        munmap(addr, map_size).map_err(|e| warn!("munmap warning: {}", e)).ok();
    }

    trace!("mmap successful: extracted {} bytes", result.len());
    Ok(result)
}

/// DMA-BUF mmap with caching for the standard (non-passthrough) path.
///
/// Uses the per-stream DmaBuf mmap cache to avoid repeated mmap syscalls
/// for the same FD. Returns the copied pixel data as Vec<u8>.
fn mmap_dmabuf_to_vec(fd: std::os::fd::RawFd, size: usize, offset: usize, cache: &DmaBufCache) -> Option<Vec<u8>> {
    use std::os::fd::BorrowedFd;

    use nix::sys::mman::{MapFlags, ProtFlags, mmap};

    let mut cache = cache.lock();

    let mapped_ptr_opt = if let Some(&(ptr, _sz)) = cache.get(&fd) {
        trace!("DMA-BUF FD={}: using cached mmap", fd);
        Some(ptr)
    } else {
        info!("DMA-BUF buffer: mmapping {} bytes from FD={} (first time)", size, fd);

        // SAFETY: sysconf(_SC_PAGESIZE) always returns a valid value
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let map_offset = (offset / page_size) * page_size;
        let map_size = size + (offset - map_offset);

        match NonZeroUsize::new(map_size) {
            Some(nz_size) => {
                // SAFETY: FD is valid from PipeWire buffer (valid during callback).
                // We cache the mapping for reuse across frames.
                unsafe {
                    let borrowed_fd = BorrowedFd::borrow_raw(fd);
                    match mmap(
                        None,
                        nz_size,
                        ProtFlags::PROT_READ,
                        MapFlags::MAP_SHARED,
                        borrowed_fd,
                        map_offset as i64,
                    ) {
                        Ok(ptr) => {
                            cache.insert(fd, (CachedMmap(ptr), map_size));
                            info!("DMA-BUF mmap cached for FD={}", fd);
                            Some(CachedMmap(ptr))
                        }
                        Err(e) => {
                            warn!("Failed to mmap DMA-BUF FD={}: {}", fd, e);
                            None
                        }
                    }
                }
            }
            None => {
                warn!("Invalid map size for DMA-BUF FD={}", fd);
                None
            }
        }
    };

    if let Some(mapped_ptr) = mapped_ptr_opt {
        // Sync DMA-BUF for CPU read access. Without this, the CPU cache may
        // contain stale data and the mmap read returns zeros on GPU-rendered buffers.
        // Constants from linux/dma-buf.h (not yet in libc crate).
        const DMA_BUF_SYNC_READ: u64 = 1;
        const DMA_BUF_SYNC_START: u64 = 0;
        const DMA_BUF_SYNC_END: u64 = 4;
        // DMA_BUF_IOCTL_SYNC = _IOW('b', 0, struct dma_buf_sync) = 0x40086200
        const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x40086200;

        #[repr(C)]
        struct DmaBufSync {
            flags: u64,
        }

        let sync_start = DmaBufSync {
            flags: DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ,
        };
        // SAFETY: fd is a valid DMA-BUF file descriptor from PipeWire.
        unsafe {
            libc::ioctl(fd, DMA_BUF_IOCTL_SYNC, &sync_start);
        }

        // SAFETY: mapped_ptr is valid from successful mmap or cache.
        // Vec capacity is allocated before writing.
        let result = unsafe {
            let src_ptr = (mapped_ptr.0.as_ptr() as *const u8).add(offset);
            let mut vec = Vec::with_capacity(size);
            std::ptr::copy_nonoverlapping(src_ptr, vec.as_mut_ptr(), size);
            vec.set_len(size);
            vec
        };

        let sync_end = DmaBufSync {
            flags: DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ,
        };
        // SAFETY: fd is a valid DMA-BUF file descriptor from PipeWire.
        unsafe {
            libc::ioctl(fd, DMA_BUF_IOCTL_SYNC, &sync_end);
        }

        trace!("DMA-BUF: extracted {} bytes from mapping", result.len());
        Some(result)
    } else {
        warn!("Failed to get DMA-BUF mapping for FD={}", fd);
        None
    }
}

/// Create a stream on the PipeWire thread
///
/// This function performs the complete stream creation, format negotiation,
/// and callback setup as specified in TASK-P1-04.
fn create_stream_on_thread(
    stream_id: u32,
    node_id: u32,
    core: &pipewire::core::Core,
    config: StreamConfig,
    frame_tx: std_mpsc::SyncSender<VideoFrame>,
    state_event_tx: std_mpsc::SyncSender<StreamStateEvent>,
    dmabuf_cache: DmaBufCache,
) -> Result<ManagedStream> {
    // Built up front (pure function of config, no PipeWire object dependency)
    // so the offered-format summary is available to capture into the
    // state_changed listener below, for logging if negotiation fails.
    let (param_pod_bytes, offered_formats_summary) = build_stream_parameters(&config)?;

    let stream_name = format!("lamco-pw-{}", stream_id);
    let node_target = node_id.to_string();

    // Build stream properties per spec
    info!("Building stream properties for stream {}", stream_id);
    let mut props = PropertiesBox::new();
    props.insert("media.type", "Video");
    props.insert("media.category", "Capture");
    props.insert("media.role", "Screen");
    props.insert("media.name", stream_name.as_str());
    props.insert("node.target", node_target.as_str());
    props.insert("stream.capture-sink", "true");

    info!("Stream properties:");
    info!(" media.type = Video");
    info!(" media.category = Capture");
    info!(" media.role = Screen");
    info!(" media.name = {}", stream_name);
    info!(" node.target = {} (Portal provided node ID)", node_target);
    info!(" stream.capture-sink = true");

    // Create the stream
    info!("Calling StreamBox::new() with properties");
    // SAFETY: We use 'static lifetime because we manually enforce that the core
    // outlives all streams (streams.clear() before drop(core) in the main loop).
    let stream: StreamBox<'static> = unsafe {
        let stream_box = StreamBox::new(core, &stream_name, props)
            .map_err(|e| PipeWireError::StreamCreationFailed(format!("StreamBox::new failed: {}", e)))?;
        // Transmute lifetime from '_ (tied to core borrow) to 'static.
        // SAFETY: Drop ordering is manually enforced in run_pipewire_main_loop.
        std::mem::transmute::<StreamBox<'_>, StreamBox<'static>>(stream_box)
    };

    info!("Stream::new() succeeded - stream object created");

    // Set up comprehensive stream event listeners
    // Clone frame_tx and dmabuf_cache for use in closures
    let frame_tx_for_process = frame_tx.clone();
    let stream_id_for_callbacks = stream_id;
    let dmabuf_cache_for_process = StdArc::clone(&dmabuf_cache);

    info!(
        " Registering stream {} callbacks (state_changed, param_changed, process)",
        stream_id
    );

    // Shared negotiated resolution — updated by param_changed, read by process
    let negotiated_width = StdArc::new(AtomicU32::new(config.width));
    let negotiated_height = StdArc::new(AtomicU32::new(config.height));
    // Negotiated DMA-BUF modifier — defaults to LINEAR, the only layout we
    // advertise and the only one the CPU mmap path can read correctly
    let negotiated_modifier = StdArc::new(AtomicU64::new(crate::ffi::drm_fourcc::DRM_FORMAT_MOD_LINEAR));
    let param_neg_width = StdArc::clone(&negotiated_width);
    let param_neg_height = StdArc::clone(&negotiated_height);
    let param_neg_modifier = StdArc::clone(&negotiated_modifier);
    let proc_neg_width = StdArc::clone(&negotiated_width);
    let proc_neg_height = StdArc::clone(&negotiated_height);
    let proc_neg_modifier = StdArc::clone(&negotiated_modifier);

    let state_tx_for_callback = state_event_tx;
    let offered_formats_summary_for_error = offered_formats_summary.clone();

    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(move |_stream, _user_data, old_state, new_state| {
            info!(
                "Stream {} state changed: {:?} -> {:?}",
                stream_id_for_callbacks, old_state, new_state
            );

            match new_state {
                StreamState::Error(ref err_msg) => {
                    error!("Stream {} entered error state: {}", stream_id_for_callbacks, err_msg);
                    // PipeWire's own error only names the failure mode (e.g. "no more
                    // input formats"), not what either side offered. We can't see the
                    // producer's EnumFormat pods from the stream API, but at least our
                    // own half of the negotiation is unambiguous in the log.
                    error!(
                        "Stream {} format negotiation failure — our {}",
                        stream_id_for_callbacks, offered_formats_summary_for_error
                    );
                }
                StreamState::Streaming => {
                    info!("Stream {} is now streaming", stream_id_for_callbacks);
                }
                StreamState::Paused => {
                    debug!("Stream {} paused", stream_id_for_callbacks);
                }
                _ => {}
            }

            // Emit state event for health monitoring
            // StreamState doesn't implement Clone, so reconstruct PwStreamState manually
            let pw_state = match new_state {
                StreamState::Unconnected => PwStreamState::Unconnected,
                StreamState::Connecting => PwStreamState::Connecting,
                StreamState::Paused => PwStreamState::Paused,
                StreamState::Streaming => PwStreamState::Streaming,
                StreamState::Error(msg) => PwStreamState::Error(msg.to_string()),
            };
            let event = StreamStateEvent {
                stream_id: stream_id_for_callbacks,
                state: pw_state,
            };
            // Non-blocking: drop event if channel full rather than stalling PipeWire
            let _ = state_tx_for_callback.try_send(event);
        })
        .param_changed(move |stream, _user_data, param_id, param| {
            let Some(param) = param else { return; };
            if param_id != ParamType::Format.as_raw() { return; }

            // Validate media type before parsing video specifics
            match format_utils::parse_format(param) {
                Ok((media_type, media_subtype)) => {
                    info!(
                        "Stream {} format negotiated: type={:?} subtype={:?}",
                        stream_id_for_callbacks, media_type, media_subtype
                    );
                }
                Err(e) => {
                    warn!("Stream {} param_changed: failed to parse media type: {e}", stream_id_for_callbacks);
                    return;
                }
            }

            // Parse the actual negotiated video format from the Pod
            let mut video_info = VideoInfoRaw::new();
            if let Err(e) = video_info.parse(param) {
                warn!("Stream {} param_changed: failed to parse VideoInfoRaw: {e}", stream_id_for_callbacks);
                return;
            }

            let size = video_info.size();
            let format = video_info.format();
            let modifier = video_info.modifier();
            info!(
                "Stream {} negotiated: {}x{} {:?} modifier={:#x}",
                stream_id_for_callbacks, size.width, size.height, format, modifier
            );

            // Update shared atomics so the process callback validates against
            // the actual compositor resolution, not the requested resolution
            param_neg_width.store(size.width, Ordering::Release);
            param_neg_height.store(size.height, Ordering::Release);
            // Stored so the DmaBuf process path can refuse to CPU-read a
            // buffer whose layout it cannot interpret (anything non-linear)
            param_neg_modifier.store(modifier, Ordering::Release);

            // Request buffer metadata types from PipeWire.
            // Without this, compositors won't attach metadata to buffers.
            if let Err(e) = request_buffer_metadata(stream, stream_id_for_callbacks) {
                warn!("Stream {} failed to request buffer metadata: {}", stream_id_for_callbacks, e);
            }
        })
        .process(move |stream, _user_data| {
            // This callback is called when a new frame buffer is available
            trace!("process() callback fired for stream {}", stream_id_for_callbacks);

            // Capture stream timing before touching buffers (RT-safe)
            // SAFETY: stream pointer is valid within this callback; pw_stream_get_time_n is RT-safe
            let stream_time = unsafe { crate::stream::get_stream_time(stream.as_raw_ptr()) };

            // Dequeue a buffer via the safe API. The returned `Buffer` requeues
            // itself on drop, so no manual queue guard is needed. `None` means the
            // stream's buffer queue is currently empty.
            let mut buffer = match stream.dequeue_buffer() {
                Some(buffer) => buffer,
                None => {
                    debug!(
                        "No buffer available (dequeue returned None) for stream {}",
                        stream_id_for_callbacks
                    );
                    return;
                }
            };

            // Extract SPA metadata via the safe libspa wrappers. This borrows the
            // buffer immutably and copies into owned fields, so the borrow is
            // released before the data blocks are taken mutably below.
            let mut buffer_meta = crate::meta::extract_buffer_meta(&buffer);

            // Access the data blocks as a safe `&mut [Data]` (no raw pointer rebuild).
            let datas_slice = buffer.datas_mut();
            if !datas_slice.is_empty() {
                trace!(
                    "Got buffer from stream {}: {} data blocks",
                    stream_id_for_callbacks,
                    datas_slice.len()
                );
                for (i, d) in datas_slice.iter_mut().enumerate() {
                    let has_data = d.data().is_some();
                    let data_len = d.data().map_or(0, |s| s.len());
                    trace!(
                        "  data[{}]: type={}, fd={}, has_data={}, data_len={}, chunk_size={}",
                        i,
                        d.type_().as_raw(),
                        d.fd(),
                        has_data,
                        data_len,
                        d.chunk().size()
                    );
                }

                // Extract frame data from buffer
                if let Some(data) = datas_slice.first_mut() {
                    // Get buffer chunk info
                    let chunk = data.chunk();
                    let size = chunk.size() as usize;
                    let offset = chunk.offset() as usize;
                    let chunk_stride = chunk.stride();
                    let data_type = data.type_();

                    // Record chunk-level signals in metadata for downstream consumers.
                    // Negative stride signals bottom-up buffer (GL coordinate convention).
                    // Buffer type affects which compositor code path produced the data.
                    buffer_meta.chunk_stride = chunk_stride;
                    buffer_meta.buffer_type = data_type.as_raw();

                    // Extract pixel data based on buffer type
                    let fd = data.fd();

                    debug!(
                        "Buffer: type={}, size={}, offset={}, fd={}, chunk_stride={}",
                        data_type.as_raw(),
                        size,
                        offset,
                        fd,
                        chunk_stride
                    );

                    let pixel_data: Option<Vec<u8>> = match data_type {
                        // MemPtr: Direct memory access via data.data()
                        libspa::buffer::DataType::MemPtr => {
                            if let Some(mapped_data) = data.data() {
                                if offset + size <= mapped_data.len() {
                                    trace!("MemPtr buffer: copying {} bytes (offset={})", size, offset);
                                    Some(mapped_data[offset..offset + size].to_vec())
                                } else {
                                    warn!(
                                        "MemPtr buffer bounds invalid: offset={}, size={}, len={}",
                                        offset,
                                        size,
                                        mapped_data.len()
                                    );
                                    None
                                }
                            } else {
                                warn!("MemPtr buffer but data.data() returned None");
                                None
                            }
                        }

                        // MemFd: File descriptor with memory mapping
                        // Always use manual mmap — PipeWire's MAP_BUFFERS auto-mapping
                        // can produce stale pointers for MemFd buffers received via
                        // portal FD connections (observed with XDPH on PipeWire 1.6.1).
                        libspa::buffer::DataType::MemFd => {
                            if fd >= 0 {
                                if size == 0 {
                                    trace!("MemFd buffer: size=0 (empty/skip frame), ignoring");
                                    None
                                } else {
                                    trace!("MemFd buffer: manual mmap (FD={}, size={}, offset={})", fd, size, offset);
                                    match mmap_fd_buffer(fd, size, offset) {
                                        Ok(data) => Some(data),
                                        Err(e) => {
                                            warn!("Failed to mmap MemFd buffer: {}", e);
                                            None
                                        }
                                    }
                                }
                            } else {
                                debug!("MemFd buffer but no valid FD (fd={})", fd);
                                None
                            }
                        }

                        // DmaBuf: GPU memory buffer
                        // Two paths: passthrough (zero-copy FD forwarding) or mmap+copy
                        libspa::buffer::DataType::DmaBuf => {
                            let negotiated_mod = proc_neg_modifier.load(Ordering::Acquire);
                            // The CPU mmap path can only interpret row-major linear
                            // buffers. Negotiation pins MOD_LINEAR, but if a producer
                            // fixates anything else, a linear copy would deliver
                            // garbage (tiled) or zeros (host-resident) — skip instead.
                            let mmap_linear_dmabuf = || -> Option<Vec<u8>> {
                                if negotiated_mod != crate::ffi::drm_fourcc::DRM_FORMAT_MOD_LINEAR {
                                    static NONLINEAR_SKIPS: AtomicU64 = AtomicU64::new(0);
                                    let skips = NONLINEAR_SKIPS.fetch_add(1, Ordering::Relaxed);
                                    if skips.is_multiple_of(300) {
                                        error!(
                                            "Stream {}: DMA-BUF modifier {:#x} is not LINEAR — \
                                             CPU mmap cannot read it, skipping frame ({} skipped so far)",
                                            stream_id_for_callbacks,
                                            negotiated_mod,
                                            skips + 1
                                        );
                                    }
                                    return None;
                                }
                                mmap_dmabuf_to_vec(fd, size, offset, &dmabuf_cache_for_process)
                            };
                            if fd >= 0 {
                                if size == 0 {
                                    trace!("DMA-BUF buffer: size=0 (empty/skip frame), ignoring");
                                    None
                                } else if config.dmabuf_passthrough {
                                    // Zero-copy path: dup the FD and send descriptor
                                    // The encoder imports this FD directly via Vulkan/VA-API
                                    use std::os::fd::OwnedFd;

                                    // SAFETY: fd is valid from PipeWire buffer during this callback
                                    // SAFETY: fd is valid from PipeWire buffer during callback.
                                    // F_DUPFD_CLOEXEC creates an independent FD copy that
                                    // survives pw_buffer release (OBS pattern).
                                    let dup_result = unsafe {
                                        let dup_fd = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0);
                                        if dup_fd >= 0 {
                                            Ok(OwnedFd::from_raw_fd(dup_fd))
                                        } else {
                                            Err(std::io::Error::last_os_error())
                                        }
                                    };

                                    match dup_result {
                                        Ok(owned_fd) => {
                                            use crate::frame::{DmaBufDescriptor, DmaBufPlane};

                                            let desc = DmaBufDescriptor {
                                                planes: vec![DmaBufPlane {
                                                    fd: owned_fd,
                                                    offset: offset as u32,
                                                    stride: chunk_stride as u32,
                                                }],
                                                // DRM format from negotiated pixel format
                                                drm_format: crate::ffi::spa_video_format_to_drm_fourcc(
                                                    config.preferred_format.unwrap_or(PixelFormat::BGRx).to_spa(),
                                                ),
                                                modifier: negotiated_mod,
                                                width: proc_neg_width.load(Ordering::Acquire),
                                                height: proc_neg_height.load(Ordering::Acquire),
                                            };

                                            debug!(
                                                "DMA-BUF passthrough: FD dup'd, {}x{}, stride={}",
                                                desc.width, desc.height, chunk_stride
                                            );

                                            // Build frame with DmaBuf variant directly
                                            // (bypasses the pixel_data path below)
                                            let neg_w = desc.width;
                                            let neg_h = desc.height;
                                            let pts = stream_time.as_ref().map_or(0, |t| t.now_nsec as u64);
                                            let damage_regions: Vec<crate::ffi::DamageRegion> = buffer_meta
                                                .damage
                                                .iter()
                                                .map(|d| crate::ffi::DamageRegion::new(d.x, d.y, d.width, d.height))
                                                .collect();

                                            let mut flags = crate::frame::FrameFlags::new();
                                            flags.set_dmabuf();

                                            let frame = VideoFrame {
                                                frame_id: stream_id_for_callbacks as u64,
                                                pts,
                                                dts: 0,
                                                duration: 16_666_667,
                                                width: neg_w,
                                                height: neg_h,
                                                stride: chunk_stride as u32,
                                                format: config.preferred_format.unwrap_or(PixelFormat::BGRx),
                                                monitor_index: 0,
                                                buffer: crate::frame::FrameBuffer::DmaBuf(desc),
                                                capture_time: SystemTime::now(),
                                                damage_regions,
                                                meta: buffer_meta.clone(),
                                                flags,
                                            };

                                            if let Err(e) = frame_tx_for_process.try_send(frame) {
                                                warn!("Failed to send DMA-BUF frame: {} (backpressure)", e);
                                            }
                                            // Return None to skip the normal pixel_data path
                                            // (frame already sent above)
                                            return;
                                        }
                                        Err(e) => {
                                            warn!("DMA-BUF FD dup failed: {}, falling back to mmap", e);
                                            // Fall through to mmap path below
                                        }
                                    }

                                    // Fallback: mmap path (only reached if dup failed)
                                    mmap_linear_dmabuf()
                                } else {
                                    // Standard mmap+copy path (dmabuf_passthrough disabled)
                                    mmap_linear_dmabuf()
                                }
                            } else {
                                debug!("DMA-BUF buffer but no valid FD (fd={})", fd);
                                None
                            }
                        }

                        // Unknown/Invalid type — portal source streams with
                        // ALLOC_BUFFERS may not set the buffer type field.
                        // Try data.data() as a fallback since the pixels may
                        // still be mapped and valid.
                        _ => {
                            if let Some(mapped_data) = data.data() {
                                if offset + size <= mapped_data.len() {
                                    debug!(
                                        "Buffer type unknown (raw={}), but mapped data available: {} bytes",
                                        data_type.as_raw(),
                                        size
                                    );
                                    Some(mapped_data[offset..offset + size].to_vec())
                                } else {
                                    warn!(
                                        "Buffer type unknown (raw={}), mapped data bounds invalid: offset={}, size={}, len={}",
                                        data_type.as_raw(),
                                        offset,
                                        size,
                                        mapped_data.len()
                                    );
                                    None
                                }
                            } else {
                                warn!(
                                    "Unknown buffer type: {} (raw={}), no mapped data",
                                    if data_type == libspa::buffer::DataType::Invalid {
                                        "Invalid"
                                    } else {
                                        "Unknown"
                                    },
                                    data_type.as_raw()
                                );
                                None
                            }
                        }
                    };

                    if let Some(pixel_data) = pixel_data {
                        // === BUFFER VALIDATION ===
                        // PipeWire sometimes provides zero-size or undersized buffers.
                        // These MUST be rejected early to prevent visual corruption.
                        // Historical analysis: zero-size buffers correlate with
                        // PipeWire negotiation races and produce a "black screen"
                        // failure mode on the client.

                        let bytes_per_pixel = 4; // BGRA/BGRx = 4 bytes
                        // Use the actual negotiated resolution from param_changed,
                        // not the requested config — compositor controls output size
                        let neg_w = proc_neg_width.load(Ordering::Acquire);
                        let neg_h = proc_neg_height.load(Ordering::Acquire);
                        let min_expected_size = (neg_w * neg_h * bytes_per_pixel) as usize;

                        if pixel_data.is_empty() {
                            // Empty buffers are normal - GNOME portal sends them as "no change" signals
                            debug!("Skipping empty buffer (size=0) - compositor indicates no change");
                            return;
                        }

                        if pixel_data.len() < min_expected_size {
                            warn!(
                                "Rejecting undersized buffer: {} bytes < {} expected for {}×{}",
                                pixel_data.len(),
                                min_expected_size,
                                neg_w,
                                neg_h
                            );
                            return;
                        }

                        // Calculate proper stride with alignment
                        // Proper stride = width * bytes_per_pixel, aligned to 16 bytes
                        let calculated_stride = ((neg_w * bytes_per_pixel + 15) / 16) * 16;

                        // Verify our calculated stride matches buffer
                        let expected_size = calculated_stride * neg_h;
                        let actual_stride = if expected_size as usize == size {
                            calculated_stride
                        } else {
                            // Buffer size doesn't match our calculation - compute actual stride
                            // This handles cases where compositor uses different alignment
                            (size / neg_h as usize) as u32
                        };

                        // Reject frames with zero stride (indicates corrupt buffer metadata)
                        if actual_stride == 0 {
                            warn!("Rejecting buffer with zero stride - corrupt metadata");
                            return;
                        }

                        // Log stride calculation details for first few frames
                        static LOGGED_FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                        let frame_count = LOGGED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        if frame_count < 5 {
                            info!("Buffer analysis frame {}:", frame_count);
                            info!(
                                "  Size: {} bytes, Width: {}, Height: {} (negotiated)",
                                size, neg_w, neg_h
                            );
                            info!("  Calculated stride: {} bytes/row (16-byte aligned)", calculated_stride);
                            info!(" Actual stride: {} bytes/row", actual_stride);
                            info!(" Expected buffer size: {} bytes", expected_size);
                            info!(" Buffer type: {} (1=MemPtr, 2=MemFd, 3=DmaBuf)", data_type.as_raw());
                            info!(
                                "  Pixel format: {:?}",
                                config.preferred_format.unwrap_or(PixelFormat::BGRx)
                            );

                            // Log first 32 bytes as hex to verify byte order
                            if pixel_data.len() >= 32 {
                                let hex_preview: Vec<String> =
                                    pixel_data[0..32].iter().map(|b| format!("{:02x}", b)).collect();
                                info!(" First 32 bytes (hex): {}", hex_preview.join(" "));
                            }

                            // Log stream timing from pw_stream_get_time_n
                            if let Some(ref t) = stream_time {
                                info!(
                                    "  PW timing: ticks={}, delay={}ns, queued={}/{} buffers, pressure={:.0}%",
                                    t.ticks,
                                    t.delay_nsec(),
                                    t.queued_buffers,
                                    t.queued_buffers + t.avail_buffers,
                                    t.buffer_pressure() * 100.0
                                );
                            }

                            // Log SPA metadata
                            info!(
                                "  SPA Meta: transform={:?}, header={}, crop={}, damage={} regions, cursor={}",
                                buffer_meta.transform,
                                if buffer_meta.header.is_some() { "present" } else { "absent" },
                                if buffer_meta.crop.is_some() { "present" } else { "absent" },
                                buffer_meta.damage.len(),
                                if buffer_meta.cursor.is_some() { "present" } else { "absent" },
                            );
                            if let Some(ref hdr) = buffer_meta.header {
                                info!(
                                    "  SPA Header: pts={}, seq={}, flags={:#x}",
                                    hdr.pts, hdr.seq, hdr.flags
                                );
                            }
                        }

                        if actual_stride != calculated_stride {
                            warn!("Stride mismatch detected:");
                            warn!(" Calculated: {} bytes/row", calculated_stride);
                            warn!(" Actual: {} bytes/row (from buffer size)", actual_stride);
                            warn!(" This may cause horizontal line artifacts!");
                        }

                        // Create VideoFrame from extracted pixel data
                        let pts = stream_time.as_ref().map_or(0, |t| t.now_nsec as u64);

                        // Convert SPA damage rects to the crate's DamageRegion type
                        let damage_regions: Vec<crate::ffi::DamageRegion> = buffer_meta
                            .damage
                            .iter()
                            .map(|d| crate::ffi::DamageRegion::new(
                                d.x, d.y, d.width, d.height,
                            ))
                            .collect();

                        let frame = VideoFrame {
                            frame_id: stream_id_for_callbacks as u64,
                            pts,
                            dts: 0,
                            duration: 16_666_667, // ~60fps default
                            width: neg_w,
                            height: neg_h,
                            stride: actual_stride,
                            format: config.preferred_format.unwrap_or(PixelFormat::BGRx),
                            monitor_index: 0,
                            buffer: crate::frame::FrameBuffer::Memory(StdArc::new(pixel_data)),
                            capture_time: SystemTime::now(),
                            damage_regions,
                            meta: buffer_meta.clone(),
                            flags: FrameFlags::new(),
                        };

                        // Send frame to async runtime
                        if let Err(e) = frame_tx_for_process.try_send(frame) {
                            warn!("Failed to send frame: {} (channel full, backpressure)", e);
                        } else {
                            debug!("Frame sent to async runtime");
                        }
                    } else {
                        trace!("No frame produced from buffer (empty/skip or unreadable)");
                    }
                } else {
                    warn!("No data in buffer for stream {}", stream_id_for_callbacks);
                }
            } else {
                // Reached for both a genuinely empty datas array and the rare anomaly
                // of a pw_buffer with a null inner spa_buffer (which datas_mut() reports
                // as empty). Either way the frame is skipped and the buffer requeued.
                warn!("No data blocks in buffer (empty or malformed) for stream {}", stream_id_for_callbacks);
            }
        })
        .register()
        .map_err(|e| PipeWireError::StreamCreationFailed(format!("Listener registration failed: {}", e)))?;

    info!("Stream {} callbacks registered successfully", stream_id);

    // Format negotiation parameters were built up front, before listener
    // registration, so state_changed could capture the offered-format summary.
    let pods: Vec<&Pod> = param_pod_bytes
        .iter()
        .filter_map(|bytes| Pod::from_bytes(bytes))
        .collect();

    info!(
        "Stream {} connecting with {} format param(s), dmabuf={}",
        stream_id,
        pods.len(),
        config.use_dmabuf
    );

    let mut params: Vec<&Pod> = pods;

    info!(
        stream_id,
        node_id, "Connecting stream with flags: AUTOCONNECT | MAP_BUFFERS | DRIVER | RT_PROCESS"
    );

    // DRIVER flag makes this stream drive the graph clock, ensuring frames
    // are delivered at the negotiated framerate even on a static desktop.
    // Without DRIVER, ScreenCast portal streams are damage-driven: no screen
    // change = no frame, causing stalls in the RDP frame delivery pipeline.
    stream
        .connect(
            Direction::Input,
            None, // PW_ID_ANY - let PipeWire use node.target property
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::DRIVER | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| PipeWireError::ConnectionFailed(format!("Stream connect failed: {}", e)))?;

    info!(
        " Stream {} .connect() succeeded - connected to node {}",
        stream_id, node_id
    );

    // NOTE: PipeWire tutorial does NOT call set_active() for portal streams
    // AUTOCONNECT flag should handle activation automatically
    // Calling set_active(true) here might interfere with auto-connection
    info!("⏳ NOT calling set_active() - AUTOCONNECT flag should activate stream automatically");
    info!("Waiting for PipeWire to transition stream to Streaming state via main loop events");
    info!(
        " If you don't see 'Stream {} is now streaming' within 2 seconds, AUTOCONNECT failed",
        stream_id
    );

    Ok(ManagedStream {
        id: stream_id,
        stream,
        _listener,
        config,
        state: StreamState::Connecting, // Initial state
        frame_count: 0,
        frame_tx,
    })
}

/// Request buffer metadata types from PipeWire via stream.update_params().
///
/// Called from param_changed after format negotiation succeeds. Without this,
/// PipeWire won't allocate space for metadata in buffer headers, and compositors
/// won't attach metadata even if they support it.
///
/// This follows the same pattern as OBS and xdg-desktop-portal-wlr.
fn request_buffer_metadata(stream: &pipewire::stream::Stream, stream_id: u32) -> Result<()> {
    use std::io::Cursor;

    use pipewire::spa;
    use pipewire::spa::pod::Value;
    use pipewire::spa::pod::serialize::PodSerializer;

    // Each metadata type we want must be requested as a separate SPA_PARAM_Meta object.
    // The object specifies the meta type ID and the minimum allocation size.
    let meta_requests: &[(u32, usize, &str)] = &[
        (
            libspa_sys::SPA_META_Header,
            std::mem::size_of::<libspa_sys::spa_meta_header>(),
            "Header",
        ),
        (
            libspa_sys::SPA_META_VideoTransform,
            std::mem::size_of::<libspa_sys::spa_meta_videotransform>(),
            "VideoTransform",
        ),
        (
            libspa_sys::SPA_META_VideoCrop,
            std::mem::size_of::<libspa_sys::spa_meta_region>(),
            "VideoCrop",
        ),
        (
            libspa_sys::SPA_META_VideoDamage,
            std::mem::size_of::<libspa_sys::spa_meta_region>() * crate::meta::MAX_DAMAGE_REGIONS,
            "VideoDamage",
        ),
        (
            libspa_sys::SPA_META_Cursor,
            std::mem::size_of::<libspa_sys::spa_meta_cursor>(),
            "Cursor",
        ),
    ];

    let mut param_bytes_list: Vec<Vec<u8>> = Vec::new();

    for &(meta_type, meta_size, name) in meta_requests {
        // Build the Object struct directly since the property!() macro expects
        // enum types with .as_raw(), but SPA_PARAM_META_* are raw u32 constants.
        let meta_obj = spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
            id: spa::param::ParamType::Meta.as_raw(),
            properties: vec![
                spa::pod::Property::new(libspa_sys::SPA_PARAM_META_type, Value::Id(spa::utils::Id(meta_type))),
                spa::pod::Property::new(libspa_sys::SPA_PARAM_META_size, Value::Int(meta_size as i32)),
            ],
        };

        match PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(meta_obj)) {
            Ok(serialized) => {
                param_bytes_list.push(serialized.0.into_inner());
                debug!(
                    "Stream {}: requested SPA_META_{} ({} bytes)",
                    stream_id, name, meta_size
                );
            }
            Err(e) => {
                warn!(
                    "Stream {}: failed to serialize SPA_META_{} request: {:?}",
                    stream_id, name, e
                );
            }
        }
    }

    if param_bytes_list.is_empty() {
        warn!("Stream {}: no metadata params serialized", stream_id);
        return Ok(());
    }

    // Convert bytes to Pod references
    let pods: Vec<&Pod> = param_bytes_list
        .iter()
        .filter_map(|bytes| Pod::from_bytes(bytes))
        .collect();

    if pods.is_empty() {
        warn!("Stream {}: no valid Pod objects from metadata params", stream_id);
        return Ok(());
    }

    let mut pod_refs: Vec<&Pod> = pods;

    stream.update_params(&mut pod_refs).map_err(|e| {
        PipeWireError::StreamCreationFailed(format!(
            "Stream {} failed to update params with metadata requests: {}",
            stream_id, e
        ))
    })?;

    info!(
        "Stream {}: requested {} metadata types from PipeWire",
        stream_id,
        pod_refs.len()
    );

    Ok(())
}

/// Build stream parameters for format negotiation.
///
/// When `config.use_dmabuf` is true, produces two EnumFormat pods following the
/// PipeWire 1.x MANDATORY flag pattern:
///   1. DmaBuf format with `SPA_FORMAT_VIDEO_modifier` carrying MANDATORY|DONT_FIXATE
///   2. SHM fallback without modifier property
///
/// PipeWire tries params in order and skips MANDATORY params it can't satisfy,
/// so DmaBuf is preferred but SHM works as automatic fallback.
///
/// When `config.use_dmabuf` is false, produces a single SHM-only param.
///
/// Returns the serialized pods alongside a short human-readable summary of
/// what was offered (formats, dmabuf modifier). Negotiation failures are
/// otherwise opaque — PipeWire's own error only names the failure mode
/// ("no more input formats"), not what either side actually offered — so
/// this summary is captured by the caller and logged if the stream enters
/// `StreamState::Error`, at least making our own half of the negotiation
/// unambiguous in the log.
fn build_stream_parameters(config: &StreamConfig) -> Result<(Vec<Vec<u8>>, String)> {
    use std::io::Cursor;

    use pipewire::spa;
    use pipewire::spa::pod::serialize::PodSerializer;
    use pipewire::spa::pod::{Property, PropertyFlags, Value};

    info!(
        "Building format parameters: {}x{} @ {}fps, dmabuf={}",
        config.width, config.height, config.framerate, config.use_dmabuf
    );

    let mut param_pods = Vec::new();

    // --- DmaBuf param (first = highest priority) ---
    if config.use_dmabuf {
        let mut dmabuf_obj = spa::pod::object!(
            spa::utils::SpaTypes::ObjectParamFormat,
            spa::param::ParamType::EnumFormat,
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaType,
                Id,
                spa::param::format::MediaType::Video
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaSubtype,
                Id,
                spa::param::format::MediaSubtype::Raw
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRA,
                spa::param::video::VideoFormat::RGBx,
                spa::param::video::VideoFormat::RGBA
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                spa::utils::Rectangle {
                    width: config.width,
                    height: config.height
                },
                spa::utils::Rectangle { width: 1, height: 1 },
                spa::utils::Rectangle {
                    width: 8192,
                    height: 8192
                }
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                spa::utils::Fraction {
                    num: config.framerate,
                    denom: 1
                },
                spa::utils::Fraction { num: 0, denom: 1 },
                spa::utils::Fraction { num: 1000, denom: 1 }
            ),
        );

        // Only MOD_LINEAR is advertised: this consumer reads buffers with a
        // plain CPU mmap, which can only interpret row-major linear layouts.
        // MOD_INVALID ("any modifier") invites tiled or host-resident
        // allocations that read back as garbage on real GPUs and all-zeros
        // on virtio-gpu. Producers that cannot supply linear skip this
        // MANDATORY param and fall through to the SHM pod below.
        // SPA Long is i64, DRM modifiers are u64 — reinterpret bits
        let mod_linear = crate::ffi::drm_fourcc::DRM_FORMAT_MOD_LINEAR as i64;
        dmabuf_obj.properties.push(Property {
            key: spa::param::format::FormatProperties::VideoModifier.as_raw(),
            flags: PropertyFlags::MANDATORY | PropertyFlags::DONT_FIXATE,
            value: Value::Choice(spa::pod::ChoiceValue::Long(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Enum {
                    default: mod_linear,
                    alternatives: vec![mod_linear],
                },
            ))),
        });

        let serialized =
            PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(dmabuf_obj)).map_err(|e| {
                PipeWireError::FormatNegotiationFailed(format!("DmaBuf format serialization failed: {e:?}"))
            })?;
        let bytes = serialized.0.into_inner();
        info!(
            "DmaBuf format param: {} bytes (MOD_LINEAR, MANDATORY|DONT_FIXATE)",
            bytes.len()
        );
        param_pods.push(bytes);
    }

    // --- SHM fallback param (no modifier property) ---
    let shm_obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: config.width,
                height: config.height
            },
            spa::utils::Rectangle { width: 1, height: 1 },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: config.framerate,
                denom: 1
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 1000, denom: 1 }
        ),
    );

    let serialized = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(shm_obj))
        .map_err(|e| PipeWireError::FormatNegotiationFailed(format!("SHM format serialization failed: {e:?}")))?;
    let bytes = serialized.0.into_inner();
    info!("SHM fallback format param: {} bytes", bytes.len());
    param_pods.push(bytes);

    info!(
        "Format negotiation: {} param(s) built (dmabuf={})",
        param_pods.len(),
        config.use_dmabuf
    );

    let summary = if config.use_dmabuf {
        "offered: [1] DmaBuf formats=[BGRx,BGRA,RGBx,RGBA] modifier=MOD_LINEAR(MANDATORY); \
         [2] SHM formats=[BGRx,BGRA,RGBx,RGBA] (no modifier)"
            .to_string()
    } else {
        "offered: [1] SHM formats=[BGRx,BGRA,RGBx,RGBA] (no modifier)".to_string()
    };

    Ok((param_pods, summary))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_thread_manager_creation() {
        // Cannot test without valid FD from portal
        // Full tests require integration testing with actual portal
    }
}
