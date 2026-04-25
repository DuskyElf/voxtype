//! `voxtype-osd-native` — native (SCTK + wgpu + egui-wgpu) on-screen
//! mic visualizer for the Voxtype daemon.
//!
//! ## Status
//!
//! Commit 3 wires up the shared IPC + ring buffer + peak-hold logic from
//! `voxtype::osd::*` and prints decoded frames to stdout. The Wayland
//! surface and rendering land in Commit 4a; until then, this binary
//! produces stdout that should match `voxtype-osd-gtk4` byte-for-byte
//! when run side-by-side against the same daemon.
//!
//! Run with `RUST_LOG=debug` for verbose logs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Parser;

use voxtype::audio::levels::{AudioFrame, FRAME_HZ};
use voxtype::osd::ipc::{resolve_socket_path, run_ipc_loop, FrameRing, DEFAULT_RING_DEPTH};
use voxtype::osd::theme::ThemeWatcher;
use voxtype::osd::visual::PeakHold;

#[derive(Parser, Debug)]
#[command(
    name = "voxtype-osd-native",
    version,
    about = "Voxtype on-screen mic visualizer (native: SCTK + wgpu + egui-wgpu)"
)]
struct Args {
    /// Path to the audio-frame Unix socket. Defaults to
    /// `$XDG_RUNTIME_DIR/voxtype/audio.sock`.
    #[arg(long, env = "VOXTYPE_OSD_SOCKET")]
    socket: Option<PathBuf>,

    /// Seconds to wait between reconnect attempts when the daemon is down.
    #[arg(long, default_value = "1.0", env = "VOXTYPE_OSD_RECONNECT_SECS")]
    reconnect_secs: f32,

    /// Print one debug line per N frames received (0 = quiet).
    #[arg(long, default_value = "100", env = "VOXTYPE_OSD_LOG_EVERY")]
    log_every: u32,

    /// Held-peak decay rate in dB/sec.
    #[arg(long, default_value = "6.0", env = "VOXTYPE_OSD_PEAK_DECAY")]
    peak_decay_db_per_sec: f32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let socket_path = resolve_socket_path(args.socket);

    tracing::info!(
        "voxtype-osd-native starting; socket={:?} (frontend=native, GUI=stub)",
        socket_path
    );

    // Theme palette is loaded once now; Commit 5 swaps in a watcher.
    let theme = ThemeWatcher::new();
    let _palette = theme.palette();

    let ring = Arc::new(Mutex::new(FrameRing::new(DEFAULT_RING_DEPTH)));
    let peak_hold = Arc::new(Mutex::new(PeakHold::new(args.peak_decay_db_per_sec)));

    let ring_for_loop = ring.clone();
    let peak_for_loop = peak_hold.clone();
    let log_every = args.log_every;

    let mut total: u64 = 0;
    let mut last_log = Instant::now();
    let dt_per_frame = 1.0 / FRAME_HZ as f32;

    let on_frame = move |frame: AudioFrame| {
        if let Ok(mut r) = ring_for_loop.lock() {
            r.push(frame);
        }
        if let Ok(mut p) = peak_for_loop.lock() {
            p.update(frame.peak_dbfs, dt_per_frame);
        }
        total += 1;
        if log_every > 0 && total.is_multiple_of(u64::from(log_every)) {
            let elapsed = last_log.elapsed().as_secs_f32();
            let rate = if elapsed > 0.0 {
                log_every as f32 / elapsed
            } else {
                0.0
            };
            let held = peak_for_loop.lock().map(|p| p.held_dbfs).unwrap_or(-120.0);
            let ring_len = ring_for_loop.lock().map(|r| r.len()).unwrap_or(0);
            tracing::debug!(
                target: "osd::frames",
                frontend = "native",
                seq = frame.seq,
                peak_dbfs = frame.peak_dbfs,
                held_dbfs = held,
                min = frame.min,
                max = frame.max,
                rate_hz = rate,
                ring_len,
                "frame batch"
            );
            last_log = Instant::now();
        }
    };

    run_ipc_loop(socket_path, args.reconnect_secs, on_frame).await
}
