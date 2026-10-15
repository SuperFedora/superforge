// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

use ratatui::style::Color;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FORGE_CATALOG: &[(&str, &str, &str, &str)] = &[
    ("superfedora", "superfedora", "rust", "16-Bit Rust Workstation Engine & TUI"),
    ("supertheme", "supertheme", "rust", "16-Bit Theme, Wallpaper, Palette & SFX Engine"),
    ("superforge", "superforge", "rust", "Hardware-Tuned Source Builder & WebApp Engine"),
    ("supershell", "superdock", "rust", "16-Bit Desktop Shell Suite (superdock..superlock)"),
    ("superagent", "superagent", "rust", "Autonomous Workstation Agent & Cast Comms Daemon"),
    ("ripgrep", "rg", "rust", "SIMD-accelerated regex search tool"),
    ("fastfetch", "fastfetch", "cmake", "Native C system info fetcher"),
    ("idlescreen", "idlescreen", "rust", "16-Bit Wayland idle screensaver daemon"),
    ("fd", "fd", "rust", "Fast parallel filesystem finder"),
    ("zoxide", "zoxide", "rust", "Smarter cd directory jumper"),
    ("btop", "btop", "cpp", "C++20 hardware & GPU monitor"),
    ("wlctl", "wlctl", "rust", "16-Bit TUI Wi-Fi / NetworkManager controller"),
    ("bluetui", "bluetui", "rust", "16-Bit TUI Bluetooth manager"),
    ("wiremix", "wiremix", "rust", "16-Bit PipeWire TUI audio mixer"),
    ("fuzzel", "fuzzel", "meson", "Wayland native app launcher"),
    ("openrgb", "openrgb", "qmake", "Open source RGB lighting control"),
    ("hyprland", "Hyprland", "cmake", "Dynamic tiling Wayland compositor"),
    ("quickshell", "quickshell", "cmake", "Qt6 Wayland shell toolkit"),
];

#[derive(Clone, Default, Deserialize)]
pub struct ForgeLive {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub pct: u16,
    #[serde(default)]
    pub step: String,
}

#[derive(Clone)]
pub struct ForgeItem {
    pub name: String,
    pub bin_name: String,
    pub kind: String,
    pub desc: String,
    pub installed: bool,
    pub size_kb: u64,
}

#[derive(Clone)]
pub struct HardwareProfile {
    pub cpu_arch: String,
    pub cpu_model: String,
    pub cpu_jobs: usize,
    pub march_target: String,
    pub isa_flags: String,
    pub linker: String,
    pub gpu_info: String,
    pub host_cflags: String,
    pub host_ldflags: String,
    pub host_rustflags: String,
}

impl HardwareProfile {
    pub fn detect() -> Self {
        let cpu_arch = env::consts::ARCH.to_string();
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let cpu_model = cpuinfo
            .lines()
            .find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string()))
            .unwrap_or_else(|| cpu_arch.clone());
        let cpu_jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let mut march_target = String::new();
        if let Ok(out) = Command::new("rustc").args(["--print", "target-cpus"]).output() {
            let txt = String::from_utf8_lossy(&out.stdout);
            for line in txt.lines() {
                if let Some(pos) = line.find("currently ") {
                    let rest = &line[pos + 10..];
                    if let Some(end) = rest.find(')') {
                        march_target = rest[..end].trim().to_string();
                        break;
                    }
                }
            }
        }
        if march_target.is_empty() {
            march_target = "native".into();
        }
        let mut flags = Vec::new();
        if let Some(fl) = cpuinfo.lines().find(|l| l.starts_with("flags")) {
            for kw in ["avx512f", "avx2", "avx", "fma", "bmi2", "aes", "vaes", "sha_ni", "sse4_2"] {
                if fl.split_whitespace().any(|w| w == kw) {
                    flags.push(kw);
                }
            }
        }
        let isa_flags = if flags.is_empty() { "standard".into() } else { flags.join(" ") };
        let (linker, ld_flag, rust_ld) = if has_bin("mold") {
            ("mold", "-fuse-ld=mold", "-C link-arg=-fuse-ld=mold")
        } else if has_bin("ld.lld") {
            ("lld", "-fuse-ld=lld", "-C link-arg=-fuse-ld=lld")
        } else {
            ("bfd", "", "")
        };
        let gpu_info = Command::new("lspci")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .find(|l| l.contains("VGA") || l.contains("3D") || l.contains("Display"))
                    .and_then(|l| l.split_once(": ").map(|(_, v)| v.trim().to_string()))
            })
            .unwrap_or_else(|| "Wayland/DRM".into());
        let host_cflags = "-O3 -pipe -march=native -mtune=native -flto=auto -ffat-lto-objects -fno-plt".to_string();
        let host_ldflags = format!("-Wl,-O1,--sort-common,--as-needed,-z,relro,-z,now -flto=auto {ld_flag}").trim().to_string();
        let host_rustflags = format!("-C opt-level=3 -C target-cpu=native -C codegen-units=1 -C strip=symbols {rust_ld}").trim().to_string();
        Self { cpu_arch, cpu_model, cpu_jobs, march_target, isa_flags, linker: linker.into(), gpu_info, host_cflags, host_ldflags, host_rustflags }
    }
}

fn has_bin(name: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {name} >/dev/null 2>&1")]).status().map(|s| s.success()).unwrap_or(false)
}

#[derive(Clone)]
pub struct ForgeState {
    pub forge_root: PathBuf,
    pub state_dir: PathBuf,
    pub hw: HardwareProfile,
    pub pri_rgb: (u8, u8, u8),
    pub sec_rgb: (u8, u8, u8),
    pub items: Vec<ForgeItem>,
    pub webapps: Vec<(String, String, String, String)>,
    pub live: ForgeLive,
}

impl ForgeState {
    pub fn load() -> Self {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
        let state_dir = env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config"))
            .join("superfedora");
        let forge_root = Self::resolve_root(&home);
        let mut st = Self {
            forge_root,
            state_dir,
            hw: HardwareProfile::detect(),
            pri_rgb: (122, 162, 247),
            sec_rgb: (187, 154, 247),
            items: Vec::new(),
            webapps: Vec::new(),
            live: ForgeLive::default(),
        };
        st.refresh();
        st
    }

    fn resolve_root(home: &str) -> PathBuf {
        if let Ok(r) = env::var("FORGE_ROOT") {
            let p = PathBuf::from(r);
            if p.join("lib/forge-runner.sh").exists() { return p; }
        }
        let proj = PathBuf::from(home).join("Projects/superfedora/superforge");
        if proj.join("lib/forge-runner.sh").exists() { proj } else { PathBuf::from(home).join(".local/share/superforge") }
    }

    pub fn primary_color(&self) -> Color { Color::Rgb(self.pri_rgb.0, self.pri_rgb.1, self.pri_rgb.2) }
    pub fn secondary_color(&self) -> Color { Color::Rgb(self.sec_rgb.0, self.sec_rgb.1, self.sec_rgb.2) }

    pub fn refresh_fast(&mut self) {
        self.live = fs::read_to_string(self.state_dir.join("forge-live.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
    }

    pub fn refresh(&mut self) {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
        let bin_dir = Path::new(&home).join(".local/bin");
        let stamp_dir = self.state_dir.join("source-built");
        self.items = FORGE_CATALOG
            .iter()
            .map(|&(name, bin_name, sys, desc)| {
                let bin_path = bin_dir.join(bin_name);
                let installed = stamp_dir.join(format!("{name}.stamp")).exists() || bin_path.exists();
                let size_kb = fs::metadata(&bin_path).map(|m| m.len() / 1024).unwrap_or(0);
                ForgeItem { name: name.into(), bin_name: bin_name.into(), kind: sys.into(), desc: desc.into(), installed, size_kb }
            })
            .collect();
        self.webapps.clear();
        if let Ok(txt) = fs::read_to_string(self.state_dir.join("forge.json"))
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt)
            && let Some(targets) = val.get("targets").and_then(|w| w.as_object())
        {
            for (slug, info) in targets {
                if info.get("type").and_then(|v| v.as_str()) == Some("webapp") {
                    let aid = info.get("app_id").and_then(|v| v.as_str()).unwrap_or(slug).to_string();
                    let name = info.get("name").and_then(|v| v.as_str()).unwrap_or(slug).to_string();
                    let prof = info.get("profile").and_then(|v| v.as_str()).unwrap_or("shared").to_string();
                    let url = info.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    self.webapps.push((aid, name, prof, url));
                }
            }
        }
        self.refresh_fast();
    }
}
