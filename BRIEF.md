# BRIEF: On-screen mic visualizer (Omarchy-themed)

## Goal

Build an opt-in on-screen visualizer that appears during recording, shows a real-time **scrolling waveform with a peak meter**, and matches the active Omarchy theme. Hyprwhspr ships a spectrum-style visualizer with GTK4; Voxtype takes a different approach — waveform + peak is more *honest* for dictation (users can tell at a glance whether their voice is being captured at a usable level, and whether they're clipping).

This is the single biggest *visual* gap between Voxtype and hyprwhspr.

## Concrete design

### Visual layout

Compact horizontal strip, ~80px tall, ~600px wide, anchored bottom-center via wlr-layer-shell. Semi-transparent background (~85% opacity) using the Omarchy theme background color.

```
┌──────────────────────────────────────────────────────┬───┐
│                                                      │ ▆ │
│        ╱╲    ╱╲╱╲       ╱╲                           │ ▆ │
│   ╱╲╱╲╱  ╲╱╲╱      ╲╱╲╱╲╱  ╲╱╲╱╲                     │ ▅ │
│   ╲╱╲╱╲╱╲╱╲╱╲      ╱╲╱╲╱╲  ╲╱╲╱                      │ ▃ │   ← peak meter
│        ╲╱    ╲╱╲╱╲       ╲╱                          │ ▁ │     (vertical, segmented)
│                                                      │   │
└──────────────────────────────────────────────────────┴───┘
   waveform (~95% width)                                  peak (~5% width)
```

- **Waveform**: scrolling left-to-right (newest on the right), mirrored around horizontal centerline. Filled polygon between min/max envelope per pixel column. Theme accent color. ~3 seconds of history visible.
- **Peak meter**: vertical segmented bar (~8–12 segments) on the right edge. Color zones: green (–∞ to –12 dBFS), yellow (–12 to –3), red (–3 to 0). Held-peak tick decays at 6 dB/sec.

### Audio data pipeline

The daemon's `src/audio/cpal_capture.rs` already has the f32 sample stream. Add an **audio-level emitter** in the same task:

- Bucket samples into fixed-rate windows: **100 Hz** (every 10 ms = 160 samples at 16 kHz mono).
- Per bucket compute: `min`, `max`, `peak_dbfs = 20*log10(max(|min|, |max|))`.
- Emit over IPC. ~16 bytes per frame, 1.6 KB/sec — negligible.

**Why 100 Hz**: matches typical 60–120 fps render rates without OSD-side interpolation; 10 ms windows are short enough to capture transient peaks honestly.

**Why min/max not RMS for the waveform**: RMS averages out transients and looks soft. Min/max preserves the visual character of speech (consonants, plosives, sibilants).

**Why peak not RMS for the meter**: clips matter; averages don't. Peak is the honest "is my mic too hot" indicator.

### IPC frame format

Binary frames, not JSON — at 100 Hz the JSON overhead is silly. Reuse the existing status socket if its protocol allows multiplexing; otherwise open a second Unix socket at `$XDG_RUNTIME_DIR/voxtype-audio.sock`.

```rust
#[repr(C)]
struct AudioFrame {
    seq: u32,         // monotonic frame counter
    min: f32,
    max: f32,
    peak_dbfs: f32,
}  // 16 bytes
```

OSD reads frames into a ring buffer (~300 entries = 3 seconds at 100 Hz) and renders from that.

### Peak-hold decay

```rust
fn update_peak_hold(current_peak: f32, held: &mut f32, dt: f32) {
    if current_peak > *held {
        *held = current_peak;
    } else {
        *held -= 6.0 * dt;  // 6 dB/sec decay
    }
}
```

### State color mapping

| State | Waveform | Peak meter | Background |
|---|---|---|---|
| Recording | theme accent | active green/yellow/red zones | theme bg @ 85% alpha |
| Transcribing | replaced with subtle progress shimmer (no live audio) | empty | same |
| Error | brief red flash, then dismiss | — | red tint, 200ms fade |
| Idle | surface destroyed | — | — |

### Configuration

```toml
[osd]
enabled = true
width_px = 600
height_px = 80
position = "bottom-center"  # bottom-center | top-center | bottom-left | bottom-right
margin_px = 24
opacity = 0.85
waveform_window_secs = 3.0
peak_decay_db_per_sec = 6.0
```

## Architecture: separate binary, NOT bolted into the daemon

The visualizer should be a **second binary** (e.g., `voxtype-osd`) that:

- Subscribes to the existing `voxtype status --follow` IPC stream
- Reads audio levels from a shared source the daemon already exposes (or a new minimal IPC if needed — keep it small)
- Reads the active Omarchy theme at startup (and ideally on theme-change signal)
- Renders a layer-shell surface that floats above all windows during the `Recording` state
- Can crash without taking down dictation

**Why separate:**
- Daemon stays lean (Voxtype principle 3: performance, especially battery on laptops)
- Users can opt out by simply not running the OSD process
- A GTK/GUI dependency in the daemon would compromise the static-binary distribution story
- If the visualizer hangs or crashes, hotkey + transcription still work

## Library / framework decision

**Recommended: `eframe` + `egui` with layer-shell glue.** Pure Rust, lightweight, drawing the waveform polygon and segmented peak bar is trivial in an immediate-mode GUI. Verify layer-shell support up front — if egui's winit backend doesn't expose it cleanly, fall back to `smithay-client-toolkit` + `wgpu` (more code but smallest binary).

Three options considered:

| Option | Pros | Cons |
|---|---|---|
| **eframe / egui** ⭐ | Pure Rust; lightweight; trivial waveform drawing | Layer-shell support depends on winit version — verify first |
| **gtk4-rs + gtk4-layer-shell** | Matches hyprwhspr's stack; mature layer-shell | Heavy deps; GObject-flavored Rust; binary size impact |
| **smithay-client-toolkit + wgpu** | Smallest binary; closest to the metal | Most code; longest path to first pixel |

Document the choice in the PR description with whatever blocker (if any) ruled out the recommended option.

## Omarchy theme integration

- Theme location: `~/.config/omarchy/current/theme/` (verify the exact path; it may be a symlink to `~/.local/share/omarchy/themes/<name>/`).
- Read the theme's color palette — typically a `colors.conf` or similar. Check Omarchy docs and existing voxtype Waybar integration for prior art on parsing this.
- Map to visualizer elements: background, foreground bars, accent / "active recording" highlight color.
- Reload on theme change: Omarchy emits a signal or writes a known file when themes switch. If detection is non-trivial, ship MVP that reads at startup and document re-launch as the workaround.

## Files to read

- `src/daemon.rs` — how `voxtype status --follow` is exposed today
- Whatever IPC mechanism the status follower uses (Unix socket? stdout? stderr?) — find by searching for `--follow`. Determines whether the audio-frame channel can multiplex onto the same socket or needs its own.
- `src/audio/cpal_capture.rs` — where the audio-level emitter goes. Must not allocate in the hot path (CLAUDE.md "Performance Considerations").
- `src/setup/waybar.rs` — Omarchy theme parsing prior art (if it exists there)
- Existing Hyprland/Sway documentation for `wlr-layer-shell` usage

## Behavior spec

- Appears at start of `Recording` state, disappears at end (including cancel path) — destroy the layer-shell surface when idle, don't just hide it
- Position and size per the `[osd]` config block above
- Click-through: surface MUST NOT steal input focus or block clicks to windows beneath (use `wlr-layer-shell` keyboard-interactivity = none, no input region)
- Multi-monitor: appears on the focused monitor; configurable via a future `output_name` setting (out of scope for v1)
- Idle CPU: zero rendering when surface is destroyed; daemon stops emitting audio frames between recordings

## Acceptance criteria

- [ ] New binary `voxtype-osd` builds (cargo workspace member or second `[[bin]]` in the existing crate)
- [ ] Daemon emits 100 Hz audio frames (`min`, `max`, `peak_dbfs`) over IPC during `Recording`; quiet otherwise
- [ ] OSD subscribes to status events AND the audio-frame channel
- [ ] Renders click-through layer-shell surface that doesn't steal focus
- [ ] Waveform: scrolls right-to-left newest, mirrored min/max envelope, theme accent color, 3-second window
- [ ] Peak meter: segmented vertical bar with green/yellow/red zones, held-peak tick with 6 dB/sec decay
- [ ] Reads Omarchy theme at startup and applies colors (background, accent, zone colors)
- [ ] Appears on `Recording`, disappears on `Idle` (surface destroyed, not just hidden), handles `Transcribing`/`Cancelled`/`Error` per the state color table
- [ ] `[osd]` config block honored; CLI flags + env vars per Voxtype principle 5
- [ ] OSD crash does not affect daemon; daemon crash causes OSD to disappear cleanly (EOF on socket)
- [ ] Idle CPU < 0.1% (verify with `perf stat` over a 60s idle window)
- [ ] Docs updated: USER_MANUAL.md "Visual feedback" section, CONFIGURATION.md for `[osd]` options, integration notes for systemd user service / Hyprland `exec-once`
- [ ] Marketing-ready screenshot in PR description (this is half the point)

## Out of scope

- Don't bake the OSD into the daemon binary
- Don't ship a spectrum analyzer / FFT view in v1 — waveform + peak only. The audio pipeline (min/max + peak_dbfs at 100 Hz) supports adding a spectrum skin later by computing FFT in the OSD; design accordingly but don't build it.
- Don't ship a custom theme system — only Omarchy. Document the path forward for non-Omarchy users (theme file at configurable path) but don't implement it.
- Don't try to support X11 — Wayland-only is consistent with Voxtype's positioning.
- Don't add per-output (per-monitor) targeting in v1; defaults to focused output.

## Open questions to resolve early

- **IPC shape**: does `voxtype status --follow` use a Unix socket, stdout, or signals? If socket, can we add a binary audio-frame channel alongside the status events, or is a second socket cleaner? Lean toward second socket for binary frames at 100 Hz — keeps the human-readable status stream clean.
- **egui layer-shell**: does the current eframe/egui release support wlr-layer-shell out of the box, or do we need a custom winit backend? If the latter, evaluate `smithay-client-toolkit + wgpu` instead before committing.
- **Daemon crash detection**: simplest path is EOF on the audio socket. OSD destroys its surface on EOF and exits (or retries with backoff). Confirm this fits the systemd user service lifecycle.
- **Meeting mode**: should the OSD show meeting-mode state (loopback active, etc.)? Match Waybar's state set for consistency — verify what Waybar shows today before deciding.
- **Theme file path**: confirm `~/.config/omarchy/current/theme/` is the canonical path on current Omarchy. Check existing Voxtype Waybar integration for prior art on parsing.
