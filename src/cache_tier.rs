// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0
// ==============================================================================
// cache_tier.rs — Wave 4 Two-Tier Hybrid Binary Cache & Background Native Re-Forge
// ==============================================================================

use crate::builder::{exec_runner, write_hw_profile};
use crate::catalog::ForgeState;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const CORE_REPOS: &[(&str, &[&str])] = &[
    ("superfedora", &["superfedora"]),
    ("superforge", &["superforge"]),
    ("supertheme", &["supertheme"]),
    ("supernotify", &["supernotify"]),
    ("supershell", &["superbar", "superdock", "supersettings", "superlock"]),
    ("superagent", &["superagent"]),
];

pub fn on_ac_power() -> bool {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else { return true };
    let mut discharging = false;
    for e in entries.flatten() {
        let p = e.path();
        if let Ok(st) = fs::read_to_string(p.join("status"))
            && st.trim().eq_ignore_ascii_case("Discharging")
        {
            discharging = true;
        }
        if let Ok(online) = fs::read_to_string(p.join("online"))
            && online.trim() == "1"
            && e.file_name().to_string_lossy().starts_with(['A', 'a'])
        {
            return true;
        }
    }
    !discharging
}

fn repo_for_target(target: &str) -> (&str, &str) {
    match target {
        "superfedora" | "sf" => ("superfedora", "superfedora"),
        "superforge" => ("superforge", "superforge"),
        "supertheme" => ("supertheme", "supertheme"),
        "supernotify" => ("supernotify", "supernotify"),
        "supershell" | "superbar" => ("supershell", "superbar"),
        "superdock" => ("supershell", "superdock"),
        "supersettings" => ("supershell", "supersettings"),
        "superlock" => ("supershell", "superlock"),
        "superagent" => ("superagent", "superagent"),
        other => (other, other),
    }
}

pub fn run_fetch_bin(st: &mut ForgeState, args: &[String]) -> i32 {
    let mut dry = false;
    let mut target = "superfedora";
    for a in args {
        match a.as_str() {
            "--dry-run" | "-n" => dry = true,
            "--quick" => {}
            "-h" | "--help" | "help" => {
                println!("Usage: superforge fetch-bin <target> [--dry-run]");
                return 0;
            }
            t => target = t,
        }
    }
    let (repo, bin_name) = repo_for_target(target);
    let url = format!(
        "https://github.com/SuperFedora/{repo}/releases/latest/download/{bin_name}-x86_64-unknown-linux-musl"
    );
    let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
    let bin_dir = PathBuf::from(&home).join(".local/bin");
    let _ = fs::create_dir_all(&bin_dir);
    let dst = bin_dir.join(bin_name);

    println!("\x1b[1;38;5;141m[::] SUPERFORGE TIER-1 QUICK BINARY CACHE ({repo} -> {bin_name})\x1b[0m");
    if dry {
        println!("  [DRY-RUN] 1. Fetch Static Musl : curl -fsSL {url}");
        println!("  [DRY-RUN] 2. Atomic Install    : {}", dst.display());
        println!("  [DRY-RUN] 3. Fallback Tier-2   : superforge build {target} (-march=native)");
        return 0;
    }

    let tmp = bin_dir.join(format!(".{bin_name}.tier1.tmp"));
    let dl_ok = Command::new("curl")
        .args(["-fsSL", "--max-time", "8", "-o"])
        .arg(&tmp)
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let valid_elf = dl_ok
        && fs::read(&tmp)
            .map(|b| b.len() > 64 && b.starts_with(b"\x7fELF"))
            .unwrap_or(false);
    if valid_elf {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
        let _ = fs::rename(&tmp, &dst);
        let _ = fs::create_dir_all(st.state_dir.join("source-built"));
        let _ = fs::write(st.state_dir.join("source-built").join(format!("{repo}.tier")), "tier1-musl\n");
        println!("  [OK] Tier-1 prebuilt static musl binary installed: {}", dst.display());
        return 0;
    }
    let _ = fs::remove_file(&tmp);

    if dst.is_file() {
        let _ = fs::create_dir_all(st.state_dir.join("source-built"));
        let _ = fs::write(st.state_dir.join("source-built").join(format!("{repo}.tier")), "tier1-cached\n");
        println!("  [OK] Tier-1 binary already present at {} (queued for Tier-2 native re-forge).", dst.display());
        return 0;
    }

    println!("  [--] Prebuilt musl binary not yet on GitHub Releases; falling back to local compilation...");
    exec_runner(st, &[target.to_string()])
}

pub fn run_background_reforge(st: &mut ForgeState, args: &[String]) -> i32 {
    let mut dry = false;
    let mut force = false;
    for a in args {
        match a.as_str() {
            "--dry-run" | "-n" => dry = true,
            "--force" | "-f" => force = true,
            "-h" | "--help" | "help" => {
                println!("Usage: superforge background-reforge [--dry-run] [--force]");
                return 0;
            }
            _ => {}
        }
    }

    let ac = on_ac_power();
    println!(
        "\x1b[1;38;5;141m[::] SUPERFORGE TIER-2 BACKGROUND NATIVE RE-FORGE (-O3 -march={} -flto=thin)\x1b[0m",
        st.hw.march_target
    );
    println!("  Power Source : {}", if ac { "AC Connected" } else { "Battery (Discharging)" });

    if !ac && !force && !dry {
        println!("  [--] Deferring Tier-2 native re-forge while on battery (use --force to override).");
        return 0;
    }

    if dry {
        println!("  [DRY-RUN] 1. Hardware Profile : CFLAGS=\"-O3 -march=native -flto=thin\" RUSTFLAGS=\"-C opt-level=3 -C target-cpu=native\"");
        println!("  [DRY-RUN] 2. Target Suite     : superfedora, superforge, supertheme, supernotify, supershell, superagent");
        println!("  [DRY-RUN] 3. Atomic Swap      : ~/.local/bin/.<bin>.reforge.tmp -> ~/.local/bin/<bin>");
        println!("  [DRY-RUN] 4. Cast Comms       : supernotify --cast builder \"BUILDER: Native Re-Forge Complete\"");
        return 0;
    }

    let _ = write_hw_profile(st);
    let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
    let ws_root = st.forge_root.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(&home).join("Projects/superfedora"));
    let bin_dir = PathBuf::from(&home).join(".local/bin");
    let _ = fs::create_dir_all(&bin_dir);
    let _ = fs::create_dir_all(st.state_dir.join("source-built"));

    for (repo, bins) in CORE_REPOS {
        let repo_dir = ws_root.join(repo);
        if !repo_dir.join("Cargo.toml").is_file() {
            continue;
        }
        println!("  [+] Re-forging {repo} (-C opt-level=3 -C target-cpu=native)...");
        let ok = Command::new("cargo")
            .current_dir(&repo_dir)
            .env("RUSTFLAGS", "-C opt-level=3 -C target-cpu=native")
            .args(["build", "--release", "-q"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            for b in *bins {
                let musl_src = repo_dir.join("target/x86_64-unknown-linux-musl/release").join(b);
                let gnu_src = repo_dir.join("target/release").join(b);
                let src = if musl_src.is_file() { musl_src } else { gnu_src };
                if src.is_file() {
                    let tmp = bin_dir.join(format!(".{b}.reforge.tmp"));
                    let dst = bin_dir.join(b);
                    if fs::copy(&src, &tmp).is_ok() {
                        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
                        let _ = fs::rename(&tmp, &dst);
                    }
                }
            }
            let _ = fs::write(st.state_dir.join("source-built").join(format!("{repo}.tier")), "tier2-native\n");
        }
    }

    let _ = Command::new("supernotify")
        .args([
            "--cast",
            "builder",
            "BUILDER: Native Re-Forge Complete",
            "Tier-2 -O3 -march=native binaries atomic-swapped into ~/.local/bin.",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    st.refresh();
    println!("  [OK] Tier-2 background -march=native re-forge complete (BUILDER Cast Comms notified).");
    0
}
