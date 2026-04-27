//! Detect `voxtype record` bindings declared in compositor configs.
//!
//! Useful when the user has the evdev listener disabled and is relying on
//! compositor-level keybindings to call voxtype. The Hotkey section's About
//! pane shows what bindings are wired up so users can verify their config
//! without leaving the TUI.
//!
//! Supports Hyprland, Sway, and Niri. Their config formats are parsed with
//! plain regex — we don't pull in a real KDL/Hyprland parser for what is
//! ultimately advisory output.
//!
//! # Compositors not yet covered
//!
//! - River: shell-script-based init; any function could call voxtype, so a
//!   simple grep would mostly produce false positives.
//! - GNOME / KDE: bindings live in dconf / kglobalshortcuts databases. Worth
//!   a follow-up but a different shape of detection.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Binding {
    pub compositor: &'static str,
    /// Human-readable key combo as written in the config (e.g. "SUPER+HOME").
    pub keys: String,
    /// Voxtype command being bound (`start`, `stop`, `toggle`, `cancel`).
    pub action: String,
    /// Path to the file the binding came from, for reporting.
    pub source: PathBuf,
}

pub fn detect() -> Vec<Binding> {
    let mut out = Vec::new();
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return out,
    };

    detect_hyprland(&home, &mut out);
    detect_sway(&home, &mut out);
    detect_niri(&home, &mut out);
    out
}

fn detect_hyprland(home: &Path, out: &mut Vec<Binding>) {
    let dir = home.join(".config/hypr");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("conf") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(b) = parse_hyprland_line(line, &path) {
                out.push(b);
            }
        }
    }
}

/// Hyprland `bindd? = MODS, KEY, NAME, exec, voxtype record ACTION` lines
/// (and `bindrd?`, `bindl`, `bindel`, `binde`, `bindle`, …).
fn parse_hyprland_line(line: &str, source: &Path) -> Option<Binding> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let (lhs, rhs) = trimmed.split_once('=')?;
    let lhs = lhs.trim();
    if !lhs.starts_with("bind") {
        return None;
    }
    if !rhs.contains("voxtype") || !rhs.contains("record") {
        return None;
    }
    // Split by commas; Hyprland tolerates whitespace.
    let parts: Vec<&str> = rhs.split(',').map(str::trim).collect();
    if parts.len() < 4 {
        return None;
    }
    let mods = parts[0];
    let key = parts[1];
    let cmd = parts.last().copied().unwrap_or("");
    let action = action_from_command(cmd)?;
    let keys = if mods.is_empty() {
        key.to_string()
    } else {
        format!("{}+{}", mods, key)
    };
    Some(Binding {
        compositor: "Hyprland",
        keys,
        action,
        source: source.to_path_buf(),
    })
}

fn detect_sway(home: &Path, out: &mut Vec<Binding>) {
    let main = home.join(".config/sway/config");
    if main.exists() {
        if let Ok(text) = fs::read_to_string(&main) {
            for line in text.lines() {
                if let Some(b) = parse_sway_line(line, &main) {
                    out.push(b);
                }
            }
        }
    }
    let conf_d = home.join(".config/sway/config.d");
    if let Ok(entries) = fs::read_dir(&conf_d) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                if let Some(b) = parse_sway_line(line, &path) {
                    out.push(b);
                }
            }
        }
    }
}

/// Sway `bindsym MOD+KEY exec voxtype record ACTION` (or `bindcode`).
fn parse_sway_line(line: &str, source: &Path) -> Option<Binding> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    if !trimmed.contains("voxtype") || !trimmed.contains("record") {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let head = parts.next()?;
    if head != "bindsym" && head != "bindcode" {
        return None;
    }
    // Skip optional `--release` and similar flags.
    let mut rest: Vec<&str> = parts.collect();
    while let Some(first) = rest.first() {
        if first.starts_with("--") {
            rest.remove(0);
        } else {
            break;
        }
    }
    let keys = rest.first()?.to_string();
    // Find `exec` and look at what comes after `voxtype record`.
    let cmd_start = rest.iter().position(|w| *w == "exec")? + 1;
    let cmd = rest[cmd_start..].join(" ");
    let action = action_from_command(&cmd)?;
    Some(Binding {
        compositor: "Sway",
        keys,
        action,
        source: source.to_path_buf(),
    })
}

fn detect_niri(home: &Path, out: &mut Vec<Binding>) {
    let path = home.join(".config/niri/config.kdl");
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    for line in text.lines() {
        if let Some(b) = parse_niri_line(line, &path) {
            out.push(b);
        }
    }
}

/// Niri's KDL `binds { Mod+Key { spawn "voxtype" "record" "ACTION"; } }`.
/// We only handle single-line bindings, which is the common case.
fn parse_niri_line(line: &str, source: &Path) -> Option<Binding> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") {
        return None;
    }
    if !trimmed.contains("voxtype") || !trimmed.contains("spawn") {
        return None;
    }
    // Form: `Mod+Key { spawn "voxtype" "record" "ACTION"; }`.
    let (keys, rest) = trimmed.split_once('{')?;
    let keys = keys.trim();
    if keys.is_empty() {
        return None;
    }
    // Pull the quoted args after `spawn`.
    let spawn_idx = rest.find("spawn")?;
    let args_part = &rest[spawn_idx + "spawn".len()..];
    let mut quoted: Vec<String> = Vec::new();
    let mut chars = args_part.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut buf = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                buf.push(c);
            }
            quoted.push(buf);
        }
    }
    if quoted.first().map(|s| s.as_str()) != Some("voxtype") {
        return None;
    }
    if quoted.get(1).map(|s| s.as_str()) != Some("record") {
        return None;
    }
    let action = quoted.get(2)?.clone();
    if !is_known_action(&action) {
        return None;
    }
    Some(Binding {
        compositor: "Niri",
        keys: keys.to_string(),
        action,
        source: source.to_path_buf(),
    })
}

fn action_from_command(cmd: &str) -> Option<String> {
    let lc = cmd.to_lowercase();
    let idx = lc.find("voxtype record")?;
    let after = &cmd[idx + "voxtype record".len()..];
    let action: String = after
        .trim_start()
        .split_whitespace()
        .next()?
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string();
    if is_known_action(&action) {
        Some(action)
    } else {
        None
    }
}

fn is_known_action(action: &str) -> bool {
    matches!(action, "start" | "stop" | "toggle" | "cancel")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dummy_path() -> &'static Path {
        Path::new("/tmp/dummy.conf")
    }

    #[test]
    fn parses_hyprland_bindd() {
        let line = "bindd  = , HOME, Voxtype PTT (start), exec, voxtype record start";
        let b = parse_hyprland_line(line, dummy_path()).unwrap();
        assert_eq!(b.compositor, "Hyprland");
        assert_eq!(b.keys, "HOME");
        assert_eq!(b.action, "start");
    }

    #[test]
    fn parses_hyprland_bindrd_with_mod() {
        let line = "bindrd = SUPER, F13, Stop, exec, voxtype record stop";
        let b = parse_hyprland_line(line, dummy_path()).unwrap();
        assert_eq!(b.keys, "SUPER+F13");
        assert_eq!(b.action, "stop");
    }

    #[test]
    fn skips_hyprland_comments_and_unrelated() {
        assert!(parse_hyprland_line("# bind = , HOME, ..., exec, voxtype record start", dummy_path()).is_none());
        assert!(parse_hyprland_line("bind = , HOME, ..., exec, alacritty", dummy_path()).is_none());
    }

    #[test]
    fn parses_sway_bindsym() {
        let line = "bindsym Mod4+Home exec voxtype record toggle";
        let b = parse_sway_line(line, dummy_path()).unwrap();
        assert_eq!(b.compositor, "Sway");
        assert_eq!(b.keys, "Mod4+Home");
        assert_eq!(b.action, "toggle");
    }

    #[test]
    fn parses_sway_with_release_flag() {
        let line = "bindsym --release Mod4+Home exec voxtype record stop";
        let b = parse_sway_line(line, dummy_path()).unwrap();
        assert_eq!(b.keys, "Mod4+Home");
        assert_eq!(b.action, "stop");
    }

    #[test]
    fn parses_niri_spawn() {
        let line = r#"    Mod+Home { spawn "voxtype" "record" "start"; }"#;
        let b = parse_niri_line(line, dummy_path()).unwrap();
        assert_eq!(b.compositor, "Niri");
        assert_eq!(b.keys, "Mod+Home");
        assert_eq!(b.action, "start");
    }

    #[test]
    fn niri_skips_other_spawn_lines() {
        let line = r#"    Mod+T { spawn "alacritty"; }"#;
        assert!(parse_niri_line(line, dummy_path()).is_none());
    }

    #[test]
    fn niri_skips_comments() {
        let line = r#"// Mod+Home { spawn "voxtype" "record" "start"; }"#;
        assert!(parse_niri_line(line, dummy_path()).is_none());
    }

    #[test]
    fn rejects_unknown_action() {
        let line = "bindd = , HOME, ..., exec, voxtype record dance";
        assert!(parse_hyprland_line(line, dummy_path()).is_none());
    }
}
