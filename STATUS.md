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

## Next

Commit 2: `voxtype-osd` binary skeleton — connect to the socket, decode frames,
log to stdout. No GUI yet.
