//! Engine-agnostic voxtype binary inventory and switching.
//!
//! Voxtype ships seven prebuilt variants in `/usr/lib/voxtype/` (Whisper:
//! avx2/avx512/vulkan; ONNX: avx2/avx512/cuda/rocm). `/usr/bin/voxtype` is a
//! symlink into that directory, and switching engines means updating that
//! symlink.
//!
//! Source builds typically live at `/usr/local/bin/voxtype` or `~/.cargo/bin/`
//! and are a single binary with whatever features were enabled at compile
//! time. They are reported as `InstallKind::Source` and switching is not
//! applicable.

use serde::Serialize;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LIB_DIR: &str = "/usr/lib/voxtype";
pub const SYSTEM_BIN: &str = "/usr/bin/voxtype";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineFamily {
    Whisper,
    Onnx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Acceleration {
    Avx2,
    Avx512,
    Vulkan,
    Cuda,
    Rocm,
    /// Source-built generic binary (no specific tier).
    Native,
}

/// Every binary name voxtype recognizes in `/usr/lib/voxtype/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Variant {
    WhisperAvx2,
    WhisperAvx512,
    WhisperVulkan,
    WhisperNative,
    OnnxAvx2,
    OnnxAvx512,
    OnnxCuda,
    OnnxRocm,
    OnnxNative,
}

impl Variant {
    pub const ALL: &'static [Variant] = &[
        Variant::WhisperAvx2,
        Variant::WhisperAvx512,
        Variant::WhisperVulkan,
        Variant::WhisperNative,
        Variant::OnnxAvx2,
        Variant::OnnxAvx512,
        Variant::OnnxCuda,
        Variant::OnnxRocm,
        Variant::OnnxNative,
    ];

    pub const fn binary_name(self) -> &'static str {
        match self {
            Variant::WhisperAvx2 => "voxtype-avx2",
            Variant::WhisperAvx512 => "voxtype-avx512",
            Variant::WhisperVulkan => "voxtype-vulkan",
            Variant::WhisperNative => "voxtype-native",
            Variant::OnnxAvx2 => "voxtype-onnx-avx2",
            Variant::OnnxAvx512 => "voxtype-onnx-avx512",
            Variant::OnnxCuda => "voxtype-onnx-cuda",
            Variant::OnnxRocm => "voxtype-onnx-rocm",
            Variant::OnnxNative => "voxtype-onnx",
        }
    }

    pub const fn family(self) -> EngineFamily {
        match self {
            Variant::WhisperAvx2
            | Variant::WhisperAvx512
            | Variant::WhisperVulkan
            | Variant::WhisperNative => EngineFamily::Whisper,
            Variant::OnnxAvx2
            | Variant::OnnxAvx512
            | Variant::OnnxCuda
            | Variant::OnnxRocm
            | Variant::OnnxNative => EngineFamily::Onnx,
        }
    }

    pub const fn acceleration(self) -> Acceleration {
        match self {
            Variant::WhisperAvx2 | Variant::OnnxAvx2 => Acceleration::Avx2,
            Variant::WhisperAvx512 | Variant::OnnxAvx512 => Acceleration::Avx512,
            Variant::WhisperVulkan => Acceleration::Vulkan,
            Variant::OnnxCuda => Acceleration::Cuda,
            Variant::OnnxRocm => Acceleration::Rocm,
            Variant::WhisperNative | Variant::OnnxNative => Acceleration::Native,
        }
    }

    pub const fn display(self) -> &'static str {
        match self {
            Variant::WhisperAvx2 => "Whisper (AVX2)",
            Variant::WhisperAvx512 => "Whisper (AVX-512)",
            Variant::WhisperVulkan => "Whisper (Vulkan)",
            Variant::WhisperNative => "Whisper (native)",
            Variant::OnnxAvx2 => "ONNX (AVX2)",
            Variant::OnnxAvx512 => "ONNX (AVX-512)",
            Variant::OnnxCuda => "ONNX (CUDA)",
            Variant::OnnxRocm => "ONNX (ROCm)",
            Variant::OnnxNative => "ONNX (native)",
        }
    }

    /// Reverse lookup. Accepts current names plus legacy `voxtype-parakeet*`
    /// names from before the ONNX rename.
    pub fn from_binary_name(name: &str) -> Option<Self> {
        match name {
            "voxtype-avx2" => Some(Variant::WhisperAvx2),
            "voxtype-avx512" => Some(Variant::WhisperAvx512),
            "voxtype-vulkan" => Some(Variant::WhisperVulkan),
            "voxtype-native" => Some(Variant::WhisperNative),
            "voxtype-onnx-avx2" | "voxtype-parakeet-avx2" => Some(Variant::OnnxAvx2),
            "voxtype-onnx-avx512" | "voxtype-parakeet-avx512" => Some(Variant::OnnxAvx512),
            "voxtype-onnx-cuda" | "voxtype-parakeet-cuda" => Some(Variant::OnnxCuda),
            "voxtype-onnx-rocm" | "voxtype-parakeet-rocm" => Some(Variant::OnnxRocm),
            "voxtype-onnx" | "voxtype-parakeet" => Some(Variant::OnnxNative),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// `/usr/bin/voxtype` resolves into `/usr/lib/voxtype/`. Switching is
    /// supported by rewriting that symlink.
    Package,
    /// The running binary lives outside `/usr/lib/voxtype/`. Single binary,
    /// switching not applicable.
    Source,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cpu {
    pub avx2: bool,
    pub avx512: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gpus {
    pub nvidia: bool,
    pub amd: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct VariantStatus {
    pub variant: Variant,
    pub binary_name: String,
    pub installed: bool,
    pub runs_on_this_cpu: bool,
    /// True if the variant has no GPU requirement, or its required GPU vendor
    /// is detected.
    pub gpu_available: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub install_kind: InstallKind,
    pub binary_path: PathBuf,
    pub package_lib_dir: Option<PathBuf>,
    pub active_variant: Option<Variant>,
    /// Empty for `InstallKind::Source`.
    pub variants: Vec<VariantStatus>,
    pub cpu: Cpu,
    pub gpus: Gpus,
    pub compiled_features: Vec<&'static str>,
}

pub fn detect_cpu() -> Cpu {
    Cpu {
        #[cfg(target_arch = "x86_64")]
        avx2: std::arch::is_x86_feature_detected!("avx2"),
        #[cfg(target_arch = "x86_64")]
        avx512: std::arch::is_x86_feature_detected!("avx512f"),
        #[cfg(not(target_arch = "x86_64"))]
        avx2: false,
        #[cfg(not(target_arch = "x86_64"))]
        avx512: false,
    }
}

pub fn detect_gpus() -> Gpus {
    Gpus {
        nvidia: detect_nvidia_gpu(),
        amd: detect_amd_gpu(),
    }
}

fn detect_nvidia_gpu() -> bool {
    if let Ok(output) = Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return true;
        }
    }
    Path::new("/dev/nvidia0").exists()
}

fn detect_amd_gpu() -> bool {
    if let Ok(output) = Command::new("lspci").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if s.contains("amd") || s.contains("radeon") {
                return true;
            }
        }
    }
    if let Ok(entries) = fs::read_dir("/dev/dri") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(num) = name.strip_prefix("renderD") {
                    let card_num = num.parse::<i32>().unwrap_or(128) - 128;
                    let vendor_path = format!("/sys/class/drm/card{}/device/vendor", card_num);
                    if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                        if vendor.trim() == "0x1002" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Path of the currently running binary, with all symlinks resolved.
pub fn current_binary_path() -> PathBuf {
    fs::read_link("/proc/self/exe").unwrap_or_else(|_| PathBuf::from(SYSTEM_BIN))
}

pub fn detect_install_kind(binary_path: &Path) -> InstallKind {
    let canonical = fs::canonicalize(binary_path).unwrap_or_else(|_| binary_path.to_path_buf());
    if canonical.starts_with(LIB_DIR) {
        InstallKind::Package
    } else {
        InstallKind::Source
    }
}

/// Read the `/usr/bin/voxtype` symlink to learn which packaged variant is
/// active. Returns `None` for source installs, missing symlinks, or unknown
/// targets.
pub fn active_variant() -> Option<Variant> {
    let target = fs::read_link(SYSTEM_BIN).ok()?;
    let name = target.file_name()?.to_str()?;
    Variant::from_binary_name(name)
}

pub fn enumerate_installed() -> Vec<Variant> {
    Variant::ALL
        .iter()
        .filter(|v| Path::new(LIB_DIR).join(v.binary_name()).exists())
        .copied()
        .collect()
}

fn variant_runs_on_cpu(v: Variant, cpu: &Cpu) -> bool {
    match v.acceleration() {
        Acceleration::Avx512 => cpu.avx512,
        // ONNX GPU binaries bundle an ONNX Runtime built with AVX-512.
        // Runtime CPU dispatch in ORT mostly handles fallback, but the
        // binary itself can still trip SIGILL on init. Treat AVX-512 as
        // a hard requirement for CUDA/ROCm variants.
        Acceleration::Cuda | Acceleration::Rocm => cpu.avx512,
        Acceleration::Avx2 | Acceleration::Vulkan | Acceleration::Native => cpu.avx2,
    }
}

fn variant_gpu_available(v: Variant, g: &Gpus) -> bool {
    match v.acceleration() {
        Acceleration::Cuda => g.nvidia,
        Acceleration::Rocm => g.amd,
        _ => true,
    }
}

pub fn compiled_features() -> Vec<&'static str> {
    let mut f = Vec::new();
    if cfg!(feature = "parakeet") {
        f.push("parakeet");
    }
    if cfg!(feature = "gpu-vulkan") {
        f.push("gpu-vulkan");
    }
    if cfg!(feature = "gpu-cuda") {
        f.push("gpu-cuda");
    }
    if cfg!(feature = "gpu-hipblas") {
        f.push("gpu-hipblas");
    }
    if cfg!(feature = "gpu-metal") {
        f.push("gpu-metal");
    }
    f
}

pub fn inventory() -> Inventory {
    let cpu = detect_cpu();
    let gpus = detect_gpus();
    let binary_path = current_binary_path();
    let install_kind = detect_install_kind(&binary_path);
    let active = active_variant();

    let variants = if install_kind == InstallKind::Package {
        Variant::ALL
            .iter()
            .map(|&v| VariantStatus {
                variant: v,
                binary_name: v.binary_name().to_string(),
                installed: Path::new(LIB_DIR).join(v.binary_name()).exists(),
                runs_on_this_cpu: variant_runs_on_cpu(v, &cpu),
                gpu_available: variant_gpu_available(v, &gpus),
                active: active == Some(v),
            })
            .collect()
    } else {
        Vec::new()
    };

    let package_lib_dir = if Path::new(LIB_DIR).is_dir() {
        Some(PathBuf::from(LIB_DIR))
    } else {
        None
    };

    Inventory {
        install_kind,
        binary_path,
        package_lib_dir,
        active_variant: active,
        variants,
        cpu,
        gpus,
        compiled_features: compiled_features(),
    }
}

/// Rewrite `/usr/bin/voxtype` to point at the requested variant's binary.
/// Requires write access to `/usr/bin/`; callers should run with sudo.
pub fn switch_to(variant: Variant) -> anyhow::Result<()> {
    let binary_path = Path::new(LIB_DIR).join(variant.binary_name());

    if !binary_path.exists() {
        anyhow::bail!(
            "Binary not found: {}\n\
             Install the appropriate voxtype package variant.",
            binary_path.display()
        );
    }

    if Path::new(SYSTEM_BIN).exists() || fs::symlink_metadata(SYSTEM_BIN).is_ok() {
        fs::remove_file(SYSTEM_BIN).map_err(|e| {
            anyhow::anyhow!(
                "Failed to remove existing symlink (need sudo?): {}\n\
                 Try: sudo voxtype setup onnx --enable",
                e
            )
        })?;
    }

    symlink(&binary_path, SYSTEM_BIN).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create symlink (need sudo?): {}\n\
             Try: sudo voxtype setup onnx --enable",
            e
        )
    })?;

    let _ = Command::new("restorecon").arg(SYSTEM_BIN).status();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_names_are_unique() {
        let mut names: Vec<&str> = Variant::ALL.iter().map(|v| v.binary_name()).collect();
        names.sort();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate binary names");
    }

    #[test]
    fn round_trip_binary_names() {
        for v in Variant::ALL {
            assert_eq!(Variant::from_binary_name(v.binary_name()), Some(*v));
        }
    }

    #[test]
    fn legacy_parakeet_names_resolve() {
        assert_eq!(
            Variant::from_binary_name("voxtype-parakeet-avx2"),
            Some(Variant::OnnxAvx2)
        );
        assert_eq!(
            Variant::from_binary_name("voxtype-parakeet-cuda"),
            Some(Variant::OnnxCuda)
        );
        assert_eq!(
            Variant::from_binary_name("voxtype-parakeet"),
            Some(Variant::OnnxNative)
        );
    }

    #[test]
    fn unknown_binary_name_is_none() {
        assert_eq!(Variant::from_binary_name("voxtype-totally-fake"), None);
        assert_eq!(Variant::from_binary_name(""), None);
    }

    #[test]
    fn family_partition() {
        let whisper = Variant::ALL
            .iter()
            .filter(|v| v.family() == EngineFamily::Whisper)
            .count();
        let onnx = Variant::ALL
            .iter()
            .filter(|v| v.family() == EngineFamily::Onnx)
            .count();
        assert_eq!(whisper, 4);
        assert_eq!(onnx, 5);
        assert_eq!(whisper + onnx, Variant::ALL.len());
    }

    #[test]
    fn cpu_gating() {
        let no_avx512 = Cpu {
            avx2: true,
            avx512: false,
        };
        assert!(variant_runs_on_cpu(Variant::WhisperAvx2, &no_avx512));
        assert!(!variant_runs_on_cpu(Variant::WhisperAvx512, &no_avx512));
        assert!(!variant_runs_on_cpu(Variant::OnnxCuda, &no_avx512));
        assert!(variant_runs_on_cpu(Variant::WhisperVulkan, &no_avx512));

        let full = Cpu {
            avx2: true,
            avx512: true,
        };
        assert!(variant_runs_on_cpu(Variant::WhisperAvx512, &full));
        assert!(variant_runs_on_cpu(Variant::OnnxCuda, &full));

        let nothing = Cpu {
            avx2: false,
            avx512: false,
        };
        assert!(!variant_runs_on_cpu(Variant::WhisperAvx2, &nothing));
        assert!(!variant_runs_on_cpu(Variant::WhisperNative, &nothing));
    }

    #[test]
    fn gpu_gating() {
        let nvidia_only = Gpus {
            nvidia: true,
            amd: false,
        };
        assert!(variant_gpu_available(Variant::OnnxCuda, &nvidia_only));
        assert!(!variant_gpu_available(Variant::OnnxRocm, &nvidia_only));
        assert!(variant_gpu_available(Variant::WhisperVulkan, &nvidia_only));

        let none = Gpus {
            nvidia: false,
            amd: false,
        };
        assert!(!variant_gpu_available(Variant::OnnxCuda, &none));
        assert!(!variant_gpu_available(Variant::OnnxRocm, &none));
        assert!(variant_gpu_available(Variant::WhisperAvx2, &none));
    }

    #[test]
    fn detect_install_kind_classifies_package_vs_source() {
        assert_eq!(
            detect_install_kind(Path::new("/usr/lib/voxtype/voxtype-avx2")),
            InstallKind::Package
        );
        assert_eq!(
            detect_install_kind(Path::new("/usr/local/bin/voxtype")),
            InstallKind::Source
        );
        assert_eq!(
            detect_install_kind(Path::new("/home/user/.cargo/bin/voxtype")),
            InstallKind::Source
        );
    }

    #[test]
    fn inventory_runs_without_panicking() {
        let inv = inventory();
        assert!(matches!(
            inv.install_kind,
            InstallKind::Package | InstallKind::Source
        ));
    }
}
