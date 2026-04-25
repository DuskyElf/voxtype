//! Parakeet/ONNX backend management.
//!
//! User-facing wrapper around [`super::binary`] for the legacy
//! `voxtype setup onnx`/`voxtype setup parakeet` CLI.

use super::binary::{self, EngineFamily, Variant};
use std::path::Path;

/// Parakeet backend variants exposed to existing callers (status formatting,
/// CLI dispatch). Each maps to one [`Variant`] in the `Onnx` family.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParakeetBackend {
    Avx2,
    Avx512,
    Cuda,
    Rocm,
    /// Custom binary (source-compiled without specific suffix)
    Custom,
}

impl ParakeetBackend {
    fn variant(self) -> Variant {
        match self {
            ParakeetBackend::Avx2 => Variant::OnnxAvx2,
            ParakeetBackend::Avx512 => Variant::OnnxAvx512,
            ParakeetBackend::Cuda => Variant::OnnxCuda,
            ParakeetBackend::Rocm => Variant::OnnxRocm,
            ParakeetBackend::Custom => Variant::OnnxNative,
        }
    }

    fn from_variant(v: Variant) -> Option<Self> {
        match v {
            Variant::OnnxAvx2 => Some(ParakeetBackend::Avx2),
            Variant::OnnxAvx512 => Some(ParakeetBackend::Avx512),
            Variant::OnnxCuda => Some(ParakeetBackend::Cuda),
            Variant::OnnxRocm => Some(ParakeetBackend::Rocm),
            Variant::OnnxNative => Some(ParakeetBackend::Custom),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.variant().display()
    }

    fn whisper_equivalent(&self) -> Variant {
        match self {
            ParakeetBackend::Avx2 => Variant::WhisperAvx2,
            ParakeetBackend::Avx512 => Variant::WhisperAvx512,
            // GPU users get Vulkan as the closest Whisper equivalent.
            ParakeetBackend::Cuda | ParakeetBackend::Rocm => Variant::WhisperVulkan,
            ParakeetBackend::Custom => Variant::WhisperNative,
        }
    }
}

/// True if the active variant is in the ONNX family.
pub fn is_parakeet_active() -> bool {
    binary::active_variant()
        .map(|v| v.family() == EngineFamily::Onnx)
        .unwrap_or(false)
}

pub fn detect_current_parakeet_backend() -> Option<ParakeetBackend> {
    ParakeetBackend::from_variant(binary::active_variant()?)
}

pub fn detect_available_backends() -> Vec<ParakeetBackend> {
    binary::enumerate_installed()
        .into_iter()
        .filter_map(ParakeetBackend::from_variant)
        .collect()
}

/// Detect which Whisper backend is currently active (legacy helper retained
/// for `show_status` output).
fn detect_current_whisper_variant() -> Option<Variant> {
    binary::active_variant().filter(|v| v.family() == EngineFamily::Whisper)
}

/// Pick the best ONNX variant for this system.
fn detect_best_parakeet_backend() -> Option<ParakeetBackend> {
    let inv = binary::inventory();
    let installed_onnx: Vec<&binary::VariantStatus> = inv
        .variants
        .iter()
        .filter(|s| s.installed && s.variant.family() == EngineFamily::Onnx)
        .collect();

    if installed_onnx.is_empty() {
        return None;
    }

    let preference = [
        Variant::OnnxCuda,
        Variant::OnnxRocm,
        Variant::OnnxAvx512,
        Variant::OnnxAvx2,
        Variant::OnnxNative,
    ];
    for v in preference {
        if let Some(status) = installed_onnx.iter().find(|s| s.variant == v) {
            if status.runs_on_this_cpu && status.gpu_available {
                return ParakeetBackend::from_variant(v);
            }
        }
    }
    // Fall back to whatever's installed even if the heuristic warns against it.
    installed_onnx
        .first()
        .and_then(|s| ParakeetBackend::from_variant(s.variant))
}

pub fn show_status() {
    println!("=== Voxtype ONNX Engine Status ===\n");

    if is_parakeet_active() {
        if let Some(backend) = detect_current_parakeet_backend() {
            println!("Active engine: Parakeet");
            println!("  Backend: {}", backend.display_name());
            println!(
                "  Binary: {}",
                Path::new(binary::LIB_DIR)
                    .join(backend.variant().binary_name())
                    .display()
            );
        }
    } else {
        println!("Active engine: Whisper");
        if let Some(variant) = detect_current_whisper_variant() {
            println!(
                "  Binary: {}",
                Path::new(binary::LIB_DIR)
                    .join(variant.binary_name())
                    .display()
            );
        }
    }

    println!("\nAvailable ONNX backends:");
    let available = detect_available_backends();
    let current = detect_current_parakeet_backend();

    if available.is_empty() {
        println!("  No ONNX binaries installed.");
        println!("\n  Install an ONNX-enabled voxtype package to use this feature.");
    } else {
        for backend in [
            ParakeetBackend::Avx2,
            ParakeetBackend::Avx512,
            ParakeetBackend::Cuda,
            ParakeetBackend::Rocm,
            ParakeetBackend::Custom,
        ] {
            let installed = available.contains(&backend);
            let active = current == Some(backend);
            let status = if active {
                "active"
            } else if installed {
                "installed"
            } else {
                "not installed"
            };
            println!("  {} - {}", backend.display_name(), status);
        }
    }

    println!();
    let gpus = binary::detect_gpus();
    let cpu = binary::detect_cpu();
    if gpus.nvidia {
        println!("NVIDIA GPU: detected");
    }
    if gpus.amd {
        println!("AMD GPU: detected");
    }
    if !gpus.nvidia && !gpus.amd {
        println!("GPU: not detected");
    }
    if (gpus.nvidia || gpus.amd) && !cpu.avx512 {
        println!("\nNote: ONNX GPU binaries (CUDA/ROCm) require AVX-512 CPU support.");
        println!("  Your CPU supports AVX2 only. Use ONNX (AVX2) for CPU-based inference,");
        println!("  or use the Whisper engine with Vulkan for GPU acceleration.");
    }

    println!();
    if !is_parakeet_active() && !available.is_empty() {
        println!("To enable ONNX engines:");
        println!("  sudo voxtype setup onnx --enable");
    } else if is_parakeet_active() {
        println!("To switch back to Whisper:");
        println!("  sudo voxtype setup onnx --disable");
    }
}

pub fn enable() -> anyhow::Result<()> {
    let available = detect_available_backends();
    if available.is_empty() {
        anyhow::bail!(
            "No ONNX binaries installed.\n\
             Install an ONNX-enabled voxtype package first."
        );
    }

    if is_parakeet_active() {
        println!("ONNX engine is already enabled.");
        if let Some(backend) = detect_current_parakeet_backend() {
            println!("  Current backend: {}", backend.display_name());
        }
        return Ok(());
    }

    let backend = detect_best_parakeet_backend()
        .ok_or_else(|| anyhow::anyhow!("No suitable ONNX backend found"))?;

    binary::switch_to(backend.variant())?;

    if super::systemd::regenerate_service_file()? {
        println!("Updated systemd service to use ONNX backend.");
    }

    println!("Switched to {} backend.", backend.display_name());
    println!();
    println!("Restart voxtype to use ONNX engines:");
    println!("  systemctl --user restart voxtype");

    Ok(())
}

pub fn disable() -> anyhow::Result<()> {
    if !is_parakeet_active() {
        println!("ONNX engine is not currently enabled (already using Whisper).");
        return Ok(());
    }

    let preferred = detect_current_parakeet_backend()
        .map(|b| b.whisper_equivalent())
        .unwrap_or(Variant::WhisperAvx2);

    let installed = binary::enumerate_installed();
    let target = if installed.contains(&preferred) {
        preferred
    } else {
        // Fall back to any installed Whisper variant in this preference order.
        let order = [
            Variant::WhisperAvx512,
            Variant::WhisperAvx2,
            Variant::WhisperVulkan,
            Variant::WhisperNative,
        ];
        let chosen = order
            .iter()
            .find(|v| installed.contains(v))
            .copied()
            .ok_or_else(|| anyhow::anyhow!("No Whisper backend found to switch to"))?;
        if chosen != preferred {
            eprintln!(
                "Note: {} not found, using {} instead",
                preferred.binary_name(),
                chosen.binary_name()
            );
        }
        chosen
    };

    binary::switch_to(target)?;

    if super::systemd::regenerate_service_file()? {
        println!("Updated systemd service to use Whisper backend.");
    }

    println!(
        "Switched to Whisper ({}) backend.",
        target.binary_name().trim_start_matches("voxtype-")
    );
    println!();
    println!("Restart voxtype to use Whisper:");
    println!("  systemctl --user restart voxtype");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_backend_round_trip() {
        for b in [
            ParakeetBackend::Avx2,
            ParakeetBackend::Avx512,
            ParakeetBackend::Cuda,
            ParakeetBackend::Rocm,
            ParakeetBackend::Custom,
        ] {
            assert_eq!(ParakeetBackend::from_variant(b.variant()), Some(b));
        }
    }

    #[test]
    fn whisper_variants_dont_resolve_to_parakeet() {
        for v in [
            Variant::WhisperAvx2,
            Variant::WhisperAvx512,
            Variant::WhisperVulkan,
            Variant::WhisperNative,
        ] {
            assert_eq!(ParakeetBackend::from_variant(v), None);
        }
    }

    #[test]
    fn whisper_equivalents_are_whisper() {
        for b in [
            ParakeetBackend::Avx2,
            ParakeetBackend::Avx512,
            ParakeetBackend::Cuda,
            ParakeetBackend::Rocm,
            ParakeetBackend::Custom,
        ] {
            assert_eq!(b.whisper_equivalent().family(), EngineFamily::Whisper);
        }
    }

    #[test]
    fn is_parakeet_active_does_not_panic() {
        let _ = is_parakeet_active();
    }

    #[test]
    fn detect_available_backends_returns_vec() {
        let backends = detect_available_backends();
        assert!(backends.len() <= 5);
    }
}
