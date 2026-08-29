# Versions & Compatibility

This workspace publishes **two parallel supported lines** of the screen-capture
crates, both built on the **PipeWire / SPA Rust bindings 0.10**, plus a legacy
0.9-era line. They differ in their system-library floor, their metadata
internals, and the capabilities they expose. Pick the line that matches your
target environment.

> All versions are MIT OR Apache-2.0, by Lamco Development LLC.
> Product pages: <https://lamco.ai/open-source/lamco-wayland/>.

## The published lines at a glance

| Line | `lamco-pipewire` | `lamco-wayland` (meta) | `lamco-video` | `lamco-portal` | PipeWire/SPA bindings | Metadata path | System libpipewire floor |
|---|---|---|---|---|---|---|---|
| **0.6.x — modern head** | **0.6.11** | **0.6.13** | **0.3.0** | **0.4.5** | 0.10 | safe `find_meta` wrappers (`unsafe`-free) | **0.3.62** |
| **0.5.x — low floor** | **0.5.11** | **0.5.12** | 0.2.0 | 0.4.5 | 0.10 | raw `libspa_sys` FFI | **0.3.33** |
| 0.4.x — legacy (0.9) | 0.4.5 | 0.4.7 | 0.1.10 | 0.4.1 | 0.9 | raw `libspa_sys` FFI | 0.3.33 |

MSRV for every current line is **Rust 1.87** (edition 2024). PipeWire 0.10 itself
only requires Rust 1.80; the 1.87 floor comes from the workspace, not the bindings.

## Which line should I use?

- **New code, or any currently-supported distribution → `0.6.x` (0.6.9).**
  This is the maintained head. Metadata extraction runs entirely through
  libspa 0.10's **safe wrappers** (the `meta.rs` module is `unsafe`-free), it
  exposes the **real cursor image** (`CursorMeta::bitmap`), and it tracks the
  current dependency set (`nix` 0.31; the meta line pulls `ashpd` 0.13.12 /
  `zbus` 5.16 through `lamco-portal` 0.4.2). It requires the system
  **libpipewire ≥ 0.3.62** (released 2022 — present on every currently-supported
  Linux distribution).

- **Older or minimal environments → `0.5.x` (0.5.9).**
  Same PipeWire 0.10 bindings and the **same DMA-BUF race fix** as 0.6.x, but a
  **lower system-library floor (libpipewire ≥ 0.3.33)** for environments that do
  not ship 0.3.62. Its metadata path uses raw `libspa_sys` FFI internally;
  behavior is functionally equivalent to 0.6.x minus the cursor-bitmap addition.

- **Still pinned to the PipeWire 0.9 bindings → `0.4.x` (0.4.5).**
  Legacy. No 0.10; kept available for consumers that have not yet migrated.

Both lines contain the DMA-BUF mmap-cache cross-thread race fix, the
DestroyStream release-ordering fix, the `pw_lifecycle` init-only lifecycle
fix (deinit is never called — it reliably segfaulted a PipeWire-internal
worker thread), the real-node-id `monitor_index` fix, the
`SPA_CHUNK_FLAG_CORRUPTED` flag fix, and the disambiguated `modifier=0x0`
negotiation log (see CHANGELOG for the complete per-version history). If you
are on an earlier patch, update within your line.

## What changed across the 0.10 program

The 0.9 → 0.10 work landed as four steps; each is a published release.

1. **0.5.0 — PipeWire/SPA bindings 0.9 → 0.10.** Clean binding migration. The
   only hard API break in the bindings was `Loop::iterate`, which now takes a
   `Timeout` enum instead of a `Duration`. Capture behavior unchanged.

2. **0.6.0 — de-FFI metadata over libspa 0.10 safe wrappers.** `meta.rs` reads
   SPA metadata (`Header` / `VideoTransform` / `VideoCrop` / `VideoDamage` /
   `Cursor`) through `Buffer::find_meta::<T>()` instead of raw pointer access —
   the module is now `unsafe`-free, and the damage-region walk is the wrapper's
   iterator. The process callback uses the safe `dequeue_buffer` / `datas_mut`
   path (auto-requeue on drop). **New capability:** `CursorMeta::bitmap`
   (`CursorBitmap` with `format` / `width` / `height` / `stride` / `pixels`) —
   the actual cursor pixels, not just an offset. The libspa feature was raised
   `v0_3_33` → `v0_3_62`, which is what lifts the 0.6.x floor to libpipewire
   0.3.62. (`SpaDataType` → re-exported `DataType`.)

3. **0.6.1 — dependency sweep.** `nix` 0.30 → 0.31 (mman API source-compatible),
   and via `lamco-portal` 0.4.2: `ashpd` 0.13.7 → 0.13.12, `zbus` → 5.16. No code
   or behavior change. `cargo-deny` clean.

4. **0.6.2 / 0.5.1 — DMA-BUF mmap-cache cross-thread race fix.** With the
   `RT_PROCESS` stream flag, the `process()` callback runs on a **separate
   realtime data-loop thread** (confirmed against the PipeWire 1.4.2 source), so
   the DMA-BUF mmap cache it shared with the main-loop stream-destroy handler via
   `Rc<RefCell<…>>` was reachable from two threads — undefined behavior. The
   cache is now `Arc<Mutex<…>>` with a `Send`-justified pointer wrapper, so every
   access is synchronized. **No API or behavior change.** Shipped to both lines
   (0.6.2 on `master`, 0.5.1 on the `release/0.5` maintenance branch).

See [`../CHANGELOG.md`](../CHANGELOG.md) for the complete per-version history.

## Capability matrix (`lamco-pipewire`)

| Capability | 0.4.x | 0.5.x | 0.6.x |
|---|---|---|---|
| PipeWire/SPA 0.10 bindings | — | ✅ | ✅ |
| DMA-BUF zero-copy (`DRM_FORMAT_MOD_LINEAR` CPU-mmap) | ✅ | ✅ | ✅ |
| YUV → BGRA (NV12 / I420 / YUY2) | ✅ | ✅ | ✅ |
| Damage-region tracking | ✅ | ✅ | ✅ |
| Cursor position / offset | ✅ | ✅ | ✅ |
| **Cursor bitmap (real pixels, `CursorMeta::bitmap`)** | — | — | ✅ |
| `unsafe`-free metadata module | — | — | ✅ |
| DMA-BUF mmap-cache race fix | — | ✅ (0.5.1) | ✅ (0.6.2) |
| DestroyStream release-ordering fix | — | ✅ (0.5.2) | ✅ (0.6.3) |

## Dependency floors by line

| Dependency | 0.5.x | 0.6.x |
|---|---|---|
| `pipewire` / `pipewire-sys` / `libspa` / `libspa-sys` | 0.10 | 0.10 |
| libspa feature flag | `v0_3_33` | `v0_3_62` |
| system **libpipewire** (build/runtime floor) | **0.3.33** | **0.3.62** |
| `nix` | 0.30 | 0.31 |
| `ashpd` (via `lamco-portal`) | 0.13.7 | 0.13.12 |
| `zbus` (via `lamco-portal`) | 5.x | 5.16 |
| Rust MSRV | 1.87 | 1.87 |

## Choosing in `Cargo.toml`

```toml
# Modern head (recommended for new code)
lamco-pipewire = "0.6"
# or the whole stack:
lamco-wayland  = "0.6"

# Low-floor line, for older/minimal libpipewire environments
lamco-pipewire = "0.5"
lamco-wayland  = "0.5"
```

Cargo's caret ranges keep you on the chosen line: `"0.6"` resolves to the latest
`0.6.z` (so you get the race fix automatically), and `"0.5"` resolves to the
latest `0.5.z`. The two lines are **not** semver-compatible with each other
(0.5 → 0.6 carries the breaking metadata-API change), so pin to one line
deliberately.
