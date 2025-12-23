# lamco-wayland Publication Status - 2025-12-23

**Repository:** https://github.com/lamco-admin/lamco-wayland
**Date:** 2025-12-23
**Status:** All crates published ✅

---

## Published Crates

| Crate | Version | Published | crates.io |
|-------|---------|-----------|-----------|
| lamco-portal | v0.2.1 | 2025-12-23 | https://crates.io/crates/lamco-portal |
| lamco-pipewire | v0.1.3 | 2025-12-23 | https://crates.io/crates/lamco-pipewire |
| lamco-video | v0.1.2 | 2025-12-23 | https://crates.io/crates/lamco-video |
| lamco-wayland (meta) | v0.2.1 | 2025-12-23 | https://crates.io/crates/lamco-wayland |

---

## Changes in This Release

### lamco-portal v0.2.1 (CRITICAL BUG FIX)

**Files:**
- `src/remote_desktop.rs` (+18, -8 lines)
- `src/session.rs` (+9, -7 lines)

**Critical Fix:**
- Changed return type from `OwnedFd` to `RawFd` in `start_session()`
- Added `std::mem::forget(fd)` to prevent FD from being closed
- Fixes black screen bug (PipeWire stream stuck in Connecting state)

**Also:**
- Enhanced debug logging for Portal session startup

**Testing:** ✅ Verified on GNOME Wayland VM

---

### lamco-pipewire v0.1.3 (IMPROVEMENTS)

**Files:**
- `src/pw_thread.rs` (+70, -31 lines)
- `src/stream.rs` (test fix)

**Changes:**
- Enhanced debug logging throughout stream lifecycle
- Removed `stream.set_active()` call (let AUTOCONNECT handle it)
- Use `PW_ID_ANY` (None) instead of explicit node_id for portal streams
- Added periodic heartbeat logging (every 1000 iterations)

**Testing:** ✅ Verified on GNOME Wayland VM

---

### lamco-video v0.1.2 (DEPENDENCY UPDATE)

**Files:**
- `Cargo.toml` (dependency update)

**Changes:**
- Updated lamco-pipewire dependency: 0.1.2 → 0.1.3

**Testing:** ✅ No code changes

---

### lamco-wayland v0.2.1 (META CRATE)

**Files:**
- `Cargo.toml` (version updates)

**Changes:**
- Updated all sub-crate dependencies to published versions

---

## Git Tags

- lamco-portal-v0.2.1
- lamco-pipewire-v0.1.3
- lamco-video-v0.1.2
- lamco-wayland-v0.2.1

---

## Commits (Clean - No Attribution)

```
1d71899 chore: update lamco-wayland meta crate dependencies
f0deaaf fix: resolve clippy warnings for publication
a0227e8 docs: update CHANGELOGs for v0.2.1, v0.1.3, v0.1.2
f619549 chore: version bumps for publication
2a2fdf5 fix(portal): Critical FD ownership fix for PipeWire stream
```

**All authored by:** Greg Lamberson <greg@lamco.io>

---

## Next Steps

**For downstream users (wrd-server-specs):**

Update to published versions:
```toml
[dependencies]
lamco-portal = { version = "0.2.1", features = ["dbus-clipboard"] }
lamco-pipewire = "0.1.3"
lamco-video = "0.1.2"
```

**Benefits:**
- Critical FD bug fix
- Better debugging with enhanced logging
- Stable crates.io versions
