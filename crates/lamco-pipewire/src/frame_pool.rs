//! Reuse of captured frames' pixel buffers.
//!
//! Every captured frame used to be copied into a new frame-sized `Vec`, allocated on PipeWire's
//! thread and usually freed on whichever thread dropped the frame last. glibc returns such a chunk
//! to the arena of the thread that allocated it but keeps it there, so a session's resident memory
//! grew with the number of threads that ever freed a frame. The pool keeps the buffers it handed
//! out and refills one that no frame holds any more, so in steady state a stream allocates nothing
//! per frame.
//!
//! A buffer is reused only when the pool's reference is the only one left, which `Arc::get_mut`
//! decides atomically. Only the pool's own thread clones its references, so once a consumer has
//! dropped a frame nothing can take the buffer back while it is being refilled.

use std::collections::VecDeque;
use std::sync::Arc;

/// How many buffers a stream keeps.
///
/// A frame is usually alive in at most three places at once (the one being encoded, the cached
/// last frame and one in the channel), so a fourth leaves the next capture a free buffer. When all
/// of them are still held the pool allocates, as every frame did before it existed.
pub(crate) const FRAME_POOL_CAPACITY: usize = 4;

/// The pixel buffers a stream has handed out, least recently used first.
pub(crate) struct FramePool {
    #[expect(
        clippy::rc_buffer,
        reason = "FrameBuffer::Memory is Arc<Vec<u8>>, and a refill needs the Vec's capacity, which Arc<[u8]> drops"
    )]
    held: VecDeque<Arc<Vec<u8>>>,
    capacity: usize,
    allocated: u64,
}

impl FramePool {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            held: VecDeque::with_capacity(capacity),
            capacity,
            allocated: 0,
        }
    }

    /// Returns a buffer that `fill` has written, or `None` when `fill` reports it couldn't.
    ///
    /// `fill` gets an empty `Vec` whose capacity is usually already frame-sized, and appends the
    /// frame's bytes to it.
    /// A buffer that `fill` failed on stays in the pool for the next frame.
    #[expect(clippy::rc_buffer, reason = "it returns what FrameBuffer::Memory holds")]
    pub(crate) fn filled(&mut self, fill: impl FnOnce(&mut Vec<u8>) -> bool) -> Option<Arc<Vec<u8>>> {
        let free = self.held.iter_mut().position(|buffer| Arc::get_mut(buffer).is_some());
        let mut buffer = match free.and_then(|index| self.held.remove(index)) {
            Some(buffer) => buffer,
            None => self.new_buffer(),
        };

        // Per the check above, or because it was just created, the pool's reference is the only one.
        let bytes = Arc::get_mut(&mut buffer).expect("a free or new buffer has no other owner");
        bytes.clear();
        let written = fill(bytes);

        // A pool without capacity keeps nothing, so each frame's buffer is its consumer's alone.
        if 0 < self.capacity {
            self.held.push_back(Arc::clone(&buffer));
        }
        written.then_some(buffer)
    }

    /// Allocates a buffer for the next frame, giving up the oldest held one when the pool is full.
    #[expect(clippy::rc_buffer, reason = "it builds what FrameBuffer::Memory holds")]
    fn new_buffer(&mut self) -> Arc<Vec<u8>> {
        if self.held.len() == self.capacity {
            // The consumer still holds this one; dropping the pool's reference leaves it theirs.
            self.held.pop_front();
        }
        self.allocated += 1;

        // With reuse on, an allocation after every slot's first fill means consumers held every buffer.
        let beyond_warm = self.allocated.saturating_sub(self.capacity as u64);
        if 0 < self.capacity && 0 < beyond_warm && beyond_warm.is_multiple_of(300) {
            tracing::debug!(
                allocated = self.allocated,
                "Capture buffers all still held by consumers, allocating"
            );
        }
        Arc::new(Vec::new())
    }

    /// How many buffers the pool has allocated, which stops growing once it is warm.
    #[cfg(test)]
    pub(crate) fn allocated(&self) -> u64 {
        self.allocated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(bytes: &'static [u8]) -> impl FnOnce(&mut Vec<u8>) -> bool {
        move |out| {
            out.extend_from_slice(bytes);
            true
        }
    }

    // oracle: mutant a pool that allocates for every frame even after the consumer has dropped the last one
    #[test]
    fn a_dropped_frame_buffer_is_refilled_for_the_next_frame() {
        let mut pool = FramePool::new(FRAME_POOL_CAPACITY);
        let first = pool.filled(write(&[1, 2, 3, 4])).expect("first frame");
        let first_address = first.as_ptr();
        drop(first);

        let second = pool.filled(write(&[5, 6, 7, 8])).expect("second frame");

        assert_eq!(second.as_ptr(), first_address);
        assert_eq!(**second, [5, 6, 7, 8]);
        assert_eq!(pool.allocated(), 1);
    }

    // oracle: mutant a pool that refills a buffer a consumer still reads
    #[test]
    fn a_frame_the_consumer_still_holds_is_never_overwritten() {
        let mut pool = FramePool::new(FRAME_POOL_CAPACITY);
        let held = pool.filled(write(&[1, 1, 1, 1])).expect("first frame");

        let next = pool.filled(write(&[2, 2, 2, 2])).expect("second frame");

        assert_eq!(**held, [1, 1, 1, 1]);
        assert_eq!(**next, [2, 2, 2, 2]);
        assert_ne!(held.as_ptr(), next.as_ptr());
    }

    // oracle: mutant a pool that keeps stale bytes when a smaller frame reuses a larger buffer
    #[test]
    fn a_smaller_frame_in_a_reused_buffer_has_only_its_own_bytes() {
        let mut pool = FramePool::new(FRAME_POOL_CAPACITY);
        drop(pool.filled(write(&[9; 8])));

        let small = pool.filled(write(&[3, 4])).expect("small frame");

        assert_eq!(**small, [3, 4]);
    }

    // oracle: mutant a pool that grows without bound while every frame it handed out is still held
    #[test]
    fn a_pool_whose_frames_are_all_held_keeps_only_its_capacity() {
        let mut pool = FramePool::new(2);
        let frames: Vec<_> = (0..5).map(|_| pool.filled(write(&[7])).expect("frame")).collect();

        assert_eq!(pool.held.len(), 2);
        assert_eq!(pool.allocated(), 5);
        // The evicted frames still belong to their holders.
        assert!(frames.iter().all(|frame| **frame == [7]));
    }

    // oracle: mutant a pool with reuse off that keeps a reference, so the consumer can't own its frame
    #[test]
    fn a_pool_without_capacity_gives_each_frame_its_own_buffer() {
        let mut pool = FramePool::new(0);
        let mut first = pool.filled(write(&[1, 2])).expect("first frame");

        assert!(Arc::get_mut(&mut first).is_some());
        drop(first);
        let second = pool.filled(write(&[3, 4])).expect("second frame");

        assert_eq!(**second, [3, 4]);
        assert!(pool.held.is_empty());
        assert_eq!(pool.allocated(), 2);
    }

    // oracle: mutant a pool that hands out a buffer a failed fill left half written, or loses it
    #[test]
    fn a_failed_fill_returns_nothing_and_keeps_the_buffer() {
        let mut pool = FramePool::new(FRAME_POOL_CAPACITY);
        drop(pool.filled(write(&[1, 2])));

        let failed = pool.filled(|out| {
            out.push(5);
            false
        });
        let next = pool.filled(write(&[6])).expect("frame after the failure");

        assert!(failed.is_none());
        assert_eq!(**next, [6]);
        assert_eq!(pool.allocated(), 1);
    }
}
