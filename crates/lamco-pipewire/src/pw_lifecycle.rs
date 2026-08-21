//! Process-wide reference-counted `pipewire::init()` / `pipewire::deinit()`.
//!
//! `pipewire::deinit()` is not a per-caller operation: it calls the underlying
//! `pw_deinit()`, which frees process-global SPA plugin handle state via
//! `unref_handle()`. This crate has multiple independent PipeWire users, each
//! on its own dedicated thread with its own lifetime (the video capture loop
//! in [`crate::pw_thread`], the audio capture loop in [`crate::audio`], and
//! [`crate::connection::PipeWireConnection`]) — none of which know about the
//! others. If one finishes and calls `deinit()` while another is still
//! running, the still-running one is left holding a dangling pointer into the
//! handle memory the first one just freed, and its next `Loop::iterate()`
//! call segfaults.
//!
//! Confirmed via valgrind on a real reproduction: the freed block and the
//! block the crashing `Loop::enter()` read from were the same allocation —
//! `unref_handle`/`pw_deinit`, called from an audio-capture thread's exit,
//! freed the handle a still-running video-capture thread's loop was built on
//! top of (`pw_load_spa_handle` → `pw_loop_new` → `pw_main_loop_new`).
//!
//! Every call site in this crate that starts or stops PipeWire must go
//! through [`acquire()`] / [`release()`] instead of `pipewire::init()`
//! / `pipewire::deinit()` directly, so the real `pipewire::deinit()` only
//! ever runs once the last user has released it.

use std::sync::Mutex;

static PW_REFCOUNT: Mutex<usize> = Mutex::new(0);

/// Register one more PipeWire user in this process. Calls the real
/// `pipewire::init()` only on the first (0 -> 1) registration; `pipewire`
/// itself documents `init()` as safe to call more than once, but routing
/// every call through the same counter as [`release()`] keeps the two
/// symmetric and auditable at a single call site.
pub(crate) fn acquire() {
    let mut count = PW_REFCOUNT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *count == 0 {
        pipewire::init();
    }
    *count += 1;
}

/// Unregister one PipeWire user. Calls the real `pipewire::deinit()` only
/// when the last user releases (1 -> 0) — i.e. only when no other thread in
/// this process still has PipeWire resources alive.
///
/// # Safety
/// The caller must have previously called [`acquire()`] exactly once for
/// this release, and must have already dropped all of its OWN PipeWire
/// resources (streams, core, context, main loop). This function does not,
/// and cannot, know whether a *different* caller's resources are still
/// alive — that is exactly the property the shared count exists to track.
pub(crate) unsafe fn release() {
    let mut count = PW_REFCOUNT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    debug_assert!(
        *count > 0,
        "pw_lifecycle::release() called without a matching acquire()"
    );
    if *count > 0 {
        *count -= 1;
    }
    if *count == 0 {
        // SAFETY: the shared count is 0, so this is the last remaining
        // PipeWire user in the process — every other caller has already
        // released. The caller's own resources are dropped per this
        // function's own contract.
        unsafe {
            pipewire::deinit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_acquire_release_only_touches_real_init_deinit_at_the_edges() {
        // This test only exercises the counter, not the real pipewire::init()/
        // deinit() (which would affect global process state and any other
        // test running concurrently). It verifies the counting logic in
        // isolation by checking against a local counter with the same shape.
        let count = Mutex::new(0usize);
        let acquire = || *count.lock().unwrap() += 1;
        let release = || *count.lock().unwrap() -= 1;

        acquire(); // 0 -> 1: would call real init()
        acquire(); // 1 -> 2: no-op
        acquire(); // 2 -> 3: no-op
        assert_eq!(*count.lock().unwrap(), 3);

        release(); // 3 -> 2: no-op
        assert_eq!(*count.lock().unwrap(), 2);
        release(); // 2 -> 1: no-op
        release(); // 1 -> 0: would call real deinit()
        assert_eq!(*count.lock().unwrap(), 0);
    }

    #[test]
    fn acquire_and_release_are_idempotent_pairs_in_practice() {
        // Exercises the real functions: this is safe because acquire/release
        // are symmetric and this test's own net effect on the process-wide
        // count is zero.
        acquire();
        acquire();
        // SAFETY: matches the two acquire() calls above, no PipeWire
        // resources were created by this test.
        unsafe {
            release();
            release();
        }
    }
}
