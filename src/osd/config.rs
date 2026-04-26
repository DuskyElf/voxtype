//! `[osd]` configuration block.
//!
//! Parsed from the user's config file alongside the rest of the daemon
//! config; can be overridden via CLI flags or `VOXTYPE_OSD_*` env vars on
//! either OSD binary. The full config layering is wired up in Commit 6.

use serde::{Deserialize, Serialize};

/// Position anchor for the OSD surface on the focused output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OsdPosition {
    #[default]
    BottomCenter,
    TopCenter,
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

/// All user-facing OSD options. Defaults match BRIEF.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OsdConfig {
    /// Run the OSD at all. When `false`, both binaries exit immediately.
    pub enabled: bool,
    /// Surface width in physical pixels.
    pub width_px: u32,
    /// Surface height in physical pixels.
    pub height_px: u32,
    /// Anchor on the focused output.
    pub position: OsdPosition,
    /// Margin from the screen edge in physical pixels.
    pub margin_px: u32,
    /// Background opacity, 0.0..=1.0.
    pub opacity: f32,
    /// Visible waveform window in seconds (3.0 per BRIEF).
    pub waveform_window_secs: f32,
    /// Held-peak decay rate in dB/sec (6.0 per BRIEF).
    pub peak_decay_db_per_sec: f32,
    /// Visual gain applied to audio samples before drawing the waveform.
    /// Mic-level voice typically peaks at ~0.1..=0.3 of full-scale; gain
    /// scales that up so the envelope fills the available height. 10.0 is
    /// the default; reduce for hot mics, increase for quiet sources.
    pub waveform_gain: f32,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            width_px: 400,
            height_px: 48,
            position: OsdPosition::BottomCenter,
            margin_px: 24,
            opacity: 0.95,
            waveform_window_secs: 3.0,
            peak_decay_db_per_sec: 6.0,
            waveform_gain: 10.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_brief() {
        let c = OsdConfig::default();
        assert!(c.enabled);
        assert_eq!(c.width_px, 400);
        assert_eq!(c.height_px, 48);
        assert_eq!(c.position, OsdPosition::BottomCenter);
        assert_eq!(c.margin_px, 24);
        assert!((c.opacity - 0.95).abs() < 1e-6);
        assert!((c.waveform_window_secs - 3.0).abs() < 1e-6);
        assert!((c.peak_decay_db_per_sec - 6.0).abs() < 1e-6);
        assert!((c.waveform_gain - 10.0).abs() < 1e-6);
    }

    #[test]
    fn position_serde_kebab_case() {
        let v: OsdPosition = serde_json::from_str("\"bottom-center\"").unwrap();
        assert_eq!(v, OsdPosition::BottomCenter);
        let v: OsdPosition = serde_json::from_str("\"top-right\"").unwrap();
        assert_eq!(v, OsdPosition::TopRight);
    }

    #[test]
    fn config_partial_toml_uses_defaults() {
        let toml_src = "width_px = 800\n";
        let c: OsdConfig = toml::from_str(toml_src).unwrap();
        assert_eq!(c.width_px, 800);
        // All other fields default
        assert_eq!(c.height_px, 48);
        assert!(c.enabled);
    }
}
