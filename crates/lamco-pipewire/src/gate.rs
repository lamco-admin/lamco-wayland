//! Frame gate
//!
//! Lets the consumer say how many of the compositor's frames it wants, so the
//! capture side can decline a frame before it copies the pixels.
//!
//! Some compositors deliver frames at their own rate whether or not anything
//! on screen changed (COSMIC streams about 47 a second to a still desktop). A
//! consumer that has seen a run of unchanged frames can ask for one frame per
//! interval instead: the copy, the damage comparison and the drop that would
//! follow never happen for the others. A frame that carries a cursor change is
//! always taken, and the consumer opens the gate again the moment it has
//! reason to expect change (its own input, a frame with damage).
//!
//! The gate only makes sense for a source that keeps sending while the screen
//! is still. A compositor that sends a frame only on damage never lets the
//! consumer see a run of unchanged frames, so the consumer never closes the
//! gate for it, and a frame it declined would not be followed by another.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Interval value meaning every frame is wanted.
const OPEN: u64 = 0;

#[derive(Debug)]
struct GateShared {
    interval_us: AtomicU64,
    /// Bumped on every `open`, so a consumer can tell the gate was opened for
    /// it from elsewhere.
    opened: AtomicU64,
    skipped: AtomicU64,
    epoch: Instant,
}

/// The consumer's end of the gate. Cheap to clone; every clone controls the
/// same gate.
#[derive(Debug, Clone)]
pub struct FrameGate {
    shared: Arc<GateShared>,
}

impl Default for FrameGate {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameGate {
    /// A gate that starts open.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(GateShared {
                interval_us: AtomicU64::new(OPEN),
                opened: AtomicU64::new(0),
                skipped: AtomicU64::new(0),
                epoch: Instant::now(),
            }),
        }
    }

    /// Take at most one frame per `interval` from each stream.
    pub fn close_to(&self, interval: Duration) {
        let micros = u64::try_from(interval.as_micros()).unwrap_or(u64::MAX).max(1);
        self.shared.interval_us.store(micros, Ordering::Release);
    }

    /// Take every frame.
    pub fn open(&self) {
        self.shared.interval_us.store(OPEN, Ordering::Release);
        self.shared.opened.fetch_add(1, Ordering::AcqRel);
    }

    pub fn is_closed(&self) -> bool {
        self.shared.interval_us.load(Ordering::Acquire) != OPEN
    }

    /// How many times `open` has been called.
    pub fn opened_count(&self) -> u64 {
        self.shared.opened.load(Ordering::Acquire)
    }

    /// Frames declined so far, across every stream using this gate.
    pub fn skipped(&self) -> u64 {
        self.shared.skipped.load(Ordering::Relaxed)
    }

    /// The capture end for one stream. Each stream keeps its own clock, so a
    /// second output isn't starved by the first.
    pub fn sampler(&self) -> GateSampler {
        GateSampler {
            shared: Arc::clone(&self.shared),
            last_admitted_us: AtomicU64::new(0),
            cursor: AtomicU64::new(0),
        }
    }
}

/// One stream's end of a [`FrameGate`].
#[derive(Debug)]
pub struct GateSampler {
    shared: Arc<GateShared>,
    last_admitted_us: AtomicU64,
    cursor: AtomicU64,
}

impl GateSampler {
    /// Whether to take this frame. `cursor` is the frame's cursor id and
    /// position when the compositor attaches one: a change in it is always
    /// taken, so the pointer stays smooth while the picture is still.
    pub fn admit(&self, cursor: Option<(u32, i32, i32)>) -> bool {
        let now = u64::try_from(self.shared.epoch.elapsed().as_micros())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let signature = cursor.map_or(0, |(id, x, y)| {
            (u64::from(id).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((u64::from(x as u32) << 32) | u64::from(y as u32)))
                | 1
        });
        let cursor_moved = self.cursor.swap(signature, Ordering::Relaxed) != signature;

        let interval = self.shared.interval_us.load(Ordering::Acquire);
        let last = self.last_admitted_us.load(Ordering::Relaxed);
        if interval == OPEN || cursor_moved || last == 0 || now.saturating_sub(last) >= interval {
            self.last_admitted_us.store(now, Ordering::Relaxed);
            return true;
        }
        self.shared.skipped.fetch_add(1, Ordering::Relaxed);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_gate_admits_every_frame() {
        let gate = FrameGate::new();
        let sampler = gate.sampler();

        assert!((0..100).all(|_| sampler.admit(None)));
        assert_eq!(gate.skipped(), 0);
    }

    #[test]
    fn a_closed_gate_admits_one_frame_per_interval() {
        let gate = FrameGate::new();
        let sampler = gate.sampler();
        assert!(sampler.admit(None));

        gate.close_to(Duration::from_millis(40));
        let burst = (0..50).filter(|_| sampler.admit(None)).count();
        assert!(burst <= 1, "{burst} frames taken in one burst");

        std::thread::sleep(Duration::from_millis(50));
        assert!(sampler.admit(None));
        assert!(!sampler.admit(None));
        assert!(gate.skipped() > 40);
    }

    #[test]
    fn a_moved_cursor_is_always_taken() {
        let gate = FrameGate::new();
        let sampler = gate.sampler();
        gate.close_to(Duration::from_secs(60));
        assert!(sampler.admit(Some((1, 10, 10))));
        assert!(!sampler.admit(Some((1, 10, 10))));

        assert!(sampler.admit(Some((1, 11, 10))));
        assert!(sampler.admit(Some((1, 11, 12))));
        assert!(!sampler.admit(Some((1, 11, 12))));
    }

    #[test]
    fn each_stream_keeps_its_own_clock() {
        let gate = FrameGate::new();
        let first = gate.sampler();
        let second = gate.sampler();
        gate.close_to(Duration::from_secs(60));

        assert!(first.admit(None));
        assert!(second.admit(None));
        assert!(!first.admit(None));
        assert!(!second.admit(None));
    }

    #[test]
    fn opening_the_gate_takes_effect_at_once_and_is_counted() {
        let gate = FrameGate::new();
        let sampler = gate.sampler();
        gate.close_to(Duration::from_secs(60));
        assert!(sampler.admit(None));
        assert!(!sampler.admit(None));
        assert!(gate.is_closed());

        let before = gate.opened_count();
        gate.open();
        assert!(!gate.is_closed());
        assert_eq!(gate.opened_count(), before + 1);
        assert!(sampler.admit(None));
        assert!(sampler.admit(None));
    }
}
