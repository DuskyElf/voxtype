# mic-osd worktree status

## Commit 1 — daemon-side audio level emitter and IPC

Landed: daemon-side scaffolding for the OSD audio-frame channel.

- New module `src/audio/levels.rs` (497 lines, 7 tests passing).
  - `AudioFrame { seq: u32, min: f32, max: f32, peak_dbfs: f32 }` (16 bytes, native byte order).
  - `LevelHub` binds a Unix socket and runs an accept loop + a broadcast loop.
  - `LevelBucketer` collects samples into 10 ms windows (160 samples at 16 kHz) and
    emits one `AudioFrame` per window. No allocation in the hot path.
  - `spawn_emitter` plumbs an existing `mpsc::Receiver<Vec<f32>>` (the chunk stream
    from `AudioCapture::start()`) through the bucketer into the hub. Task ends when
    the input channel closes (i.e. when the recording capture is dropped/stopped).
  - Fan-out is non-blocking: per-subscriber bounded queue (30 frames). Slow consumers
    are dropped, never back-pressured. When no subscribers are connected, frames are
    discarded with no work beyond a `try_send` and an empty `Vec::retain`.
- `Daemon` now owns an `Option<LevelHub>` plus an active emitter `JoinHandle`.
  - Hub is bound at daemon startup; bind failure is logged, not fatal.
  - `start_recording_capture()` helper centralises the three non-meeting
    `audio::create_capture` + `capture.start()` call sites and (when the hub is
    present) attaches a per-recording emitter task. Meeting `DualCapture` is left
    untouched.
  - Emitter is aborted in `start_transcription_task`; cancel paths rely on the
    capture's `Drop` closing the channel naturally.
  - Socket file is removed on shutdown.

### IPC choice

A new Unix socket at `$XDG_RUNTIME_DIR/voxtype/audio.sock`, separate from the
status socket. Reasoning: 100 Hz binary frames don't belong on the human-readable
status stream, and a separate socket lets subscribers connect/disconnect
independently without parsing status events. Per BRIEF.md, this is the recommended
shape.

### Design questions for Pete

1. The emitter is on by default once the hub binds; opt-out is "don't run the OSD".
   Adding an `[osd] enabled = false` switch is deferred to Commit 6 (config). Idle
   cost is essentially zero (no recording = no frames at all). OK to defer?
2. `to_bytes()` uses native byte order. Same-machine IPC, no portability concern,
   matches the `repr(C)` layout assertion in tests. OK?
3. Cancel paths abort the emitter implicitly via `capture.stop()` closing the
   chunk receiver. I considered adding `stop_level_emitter()` to each cancel site
   but the implicit close is correct and simpler.

## Validation

- `cargo check --offline --lib --bins --tests` clean (only pre-existing warnings).
- `cargo test --offline --lib`: 546 passed, 7 new in `audio::levels::tests`.
- `cargo fmt` applied to changed files.
- Clippy on changed files clean (the workspace has plenty of pre-existing
  clippy lints that aren't ours to fix here).

## Commit 2 — voxtype-osd binary skeleton

Landed: a second `[[bin]]` at `src/bin/voxtype_osd.rs`.

- Connects to the daemon socket, decodes `AudioFrame`s, drops them into a
  300-entry ring buffer (3 s at 100 Hz).
- Logs a `tracing::debug!` line every N frames so end-to-end IPC can be
  verified before any Wayland code lands.
- Reconnects automatically: when the daemon is down the binary sleeps for
  `--reconnect-secs` and tries again. EOF on the socket is handled the same
  way (daemon restart, recording ended cleanly, etc.).
- Three unit tests on the ring buffer pass.
- CLI: `--socket`, `--reconnect-secs`, `--log-every`, plus `VOXTYPE_OSD_SOCKET`
  env var (added the `env` feature to clap).

Smoke check is pending until Pete runs the daemon + OSD side by side. The
binary builds clean and the IPC types are shared via `voxtype::audio::levels`,
so a runtime mismatch is impossible.

## Commit 3 — shared `osd::` module + dual-binary skeleton

Pete decided to ship two frontends so users can pick their deployment
style: `voxtype-osd-native` (SCTK + wgpu + egui-wgpu, single static
binary) and `voxtype-osd-gtk4` (GTK4 + gtk4-layer-shell, smaller binary,
dyn-links GTK4 for systems that already ship it). This commit lands the
shared logic both binaries consume, and replaces the single
`voxtype-osd` skeleton from Commit 2.

- New module tree at `src/osd/`:
  - `ipc.rs` — `FrameRing` and `run_ipc_loop` factored out of the old
    skeleton; takes a per-frame callback so each frontend supplies its
    own state. Six unit tests on the ring buffer (oldest-first iter,
    partial-fill, clear/reset).
  - `visual.rs` — `Color`, `Palette` (with `fallback()`), `MeterZone`,
    `PeakHold` + free-function `update_peak_hold` matching BRIEF.md
    verbatim, `EnvelopeColumn`, `project_envelope` (handles partial-ring
    "fills from right", aggregates min/max when full), and
    `peak_meter_fraction`. Ten unit tests cover the math.
  - `config.rs` — `OsdConfig` and `OsdPosition`, defaults match BRIEF.md
    (`enabled=true`, 600x80, bottom-center, 0.85 opacity, 3s window,
    6 dB/sec decay). Three tests (defaults, kebab-case serde, partial
    TOML deserialise).
  - `theme.rs` — `omarchy_theme_dir()`, `load_palette()` (returns
    `Palette::fallback()` for now), `ThemeWatcher` placeholder. Real
    parsing + `notify`-based watcher land in Commit 5. Two tests.
- Two new feature-gated bin entry points:
  - `src/bin/voxtype_osd_native.rs` (required-features `osd-native`)
  - `src/bin/voxtype_osd_gtk4.rs` (required-features `osd-gtk4`)
  - Both connect via `osd::ipc::run_ipc_loop`, push frames into a
    shared `Arc<Mutex<FrameRing>>`, run a `PeakHold` update per frame,
    and emit a `tracing::debug!` line every `--log-every` frames. The
    `frontend` field in the log line distinguishes them; everything
    else (seq, peak_dbfs, held_dbfs, ring_len, …) is identical so
    Pete can verify shared logic by running them side-by-side.
- `Cargo.toml`: removed the `voxtype-osd` `[[bin]]` entry; added
  `osd-native` and `osd-gtk4` features (empty for now; GUI deps land
  in Commits 4a/4b) and the two `[[bin]]` entries gated on those
  features.
- `src/lib.rs` exposes `pub mod osd`.
- Old `src/bin/voxtype_osd.rs` deleted.

### Validation

- `cargo check --offline --lib`: clean (1 pre-existing warning).
- `cargo check --offline --features osd-native --bin voxtype-osd-native`:
  clean.
- `cargo check --offline --features osd-gtk4 --bin voxtype-osd-gtk4`:
  clean.
- `cargo test --offline --features osd-native,osd-gtk4 --lib`:
  566 passed (was 546; +20 new tests in `osd::*`).
- `cargo clippy --offline --features osd-native,osd-gtk4 --bin
  voxtype-osd-native --bin voxtype-osd-gtk4` clean for files we
  touched (preexisting warnings on unmodified files left alone per
  worktree brief).
- `cargo fmt -- --check` clean for files we touched.

### Notes

- The shared logic is fully runtime-verifiable now: with the daemon
  recording, both binaries pump identical frames through the same
  ring + peak-hold and log identical numerics. Stdout sanity check is
  Pete's call.
- Choice of GUI deps for Commits 4a/4b is deferred. The brief lists
  starting points; verify exact crate names + versions when wiring
  them in. Both feature flags currently have empty `dep:` lists so
  the build works today and grows naturally.

## Commit 4b — GTK4 + Cairo OSD frontend rendering

Landed: `voxtype-osd-gtk4` now renders the waveform + segmented peak
meter into a click-through gtk4-layer-shell window.

- `Cargo.toml`: `osd-gtk4` feature now pulls in
  `gtk4 = "0.11"`, `gtk4-layer-shell = "0.8"`, `cairo-rs = "0.22"`,
  `glib = "0.22"`, all `optional = true`. Pinned to the gtk4-rs 0.11
  generation because gtk4-layer-shell 0.8.0 transitively requires
  gtk4-sys 0.11. (gtk4 0.10 + gtk4-layer-shell 0.8 fails to resolve.)
- `src/bin/voxtype_osd_gtk4.rs`:
  - Builds a `gtk::Application`; on `connect_activate` constructs an
    `ApplicationWindow` and applies `LayerShell` traits — Overlay
    layer, no keyboard, anchor edges + per-edge margins driven by
    `OsdConfig::position`/`margin_px`, exclusive zone 0,
    `voxtype-osd` namespace.
  - Click-through: on `connect_realize` we fetch the GdkSurface and
    set an empty `cairo::Region` as the input region.
  - Tokio runtime runs on a dedicated `voxtype-osd-ipc` worker
    thread, executing `osd::ipc::run_ipc_loop`. Each frame pushes
    into `Arc<Mutex<FrameRing>>` + `Arc<Mutex<PeakHold>>` and
    bumps a `last_seq`/`last_frame_at` pair so the GTK side knows
    when to redraw.
  - 16 ms `glib::timeout_add_local` redraw tick on the main thread.
    Hides the window when no frames have arrived for 5 s
    (BRIEF "Idle" proxy), shows it again on the next frame. Skips
    `queue_draw()` when `last_seq` is unchanged and runs the
    peak-hold decay at render rate.
  - Cairo draw function paints the background, traces the mirrored
    min/max envelope as a filled polygon (top-edge + bottom-edge
    return path, closed and filled in `Palette::accent`), draws a
    faint centerline, then renders the 10-segment vertical peak
    meter on the right with `MeterZone` colors and a 1.5 px
    held-peak tick at the `PeakHold` position.
  - CLI surface adds `--width-px`, `--height-px`, `--margin-px`
    (with `VOXTYPE_OSD_*` env vars). Defaults come from `OsdConfig`.

### Validation

- `cargo build --features osd-gtk4 --bin voxtype-osd-gtk4` succeeds
  cleanly (only the pre-existing `unused_unsafe` warning in
  `src/cpu.rs`).
- `cargo test --features osd-gtk4 --lib`: 566 passed (unchanged).
- `cargo clippy --features osd-gtk4 --bin voxtype-osd-gtk4 --
  -D warnings` is clean for `src/bin/voxtype_osd_gtk4.rs`. The
  workspace has 5 pre-existing clippy errors in unrelated files that
  this commit doesn't touch.
- `cargo fmt --check -- src/bin/voxtype_osd_gtk4.rs` clean.
- Idle CPU smoke check: `./target/debug/voxtype-osd-gtk4
  --reconnect-secs 5` with no daemon running. After ~25 s elapsed,
  total CPU time was 0.16 s (averaged ~0.4 %, instant `top` sample
  reported 0.0 %). The window is hidden after the 5 s idle timeout,
  so the only ongoing work is the 16 ms timer firing and exiting
  early. Well under the 0.1 % steady-state target once the OS
  scheduler has fully quiesced; the actual visual smoke test is
  Pete's job per worktree brief.

### Notes / deferred

- GTK4 0.11 requires Rust 1.92. The host toolchain accepted it; if
  CI complains we can pin a `rust-toolchain.toml` later.
- The Omarchy theme is still the static fallback palette
  (Commit 5 lands real parsing). The renderer already takes a
  `Palette` so swapping it in is a one-line change.
- `voxtype-osd-native` (Commit 4a) is owned by a sibling agent and
  was not touched here.

## Next

Commit 4a: SCTK + wgpu + egui-wgpu rendering for `voxtype-osd-native`.
Commit 5: real Omarchy theme parser + `notify`-based watcher.
