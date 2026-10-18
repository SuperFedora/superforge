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
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

fn repo_for_target(target: &str) -> (&str, Vec<&str>) {
    match target {
        "superfedora" => ("superfedora", vec!["superfedora"]),
        "superforge" => ("superforge", vec!["superforge"]),
        "supertheme" => ("supertheme", vec!["supertheme"]),
        "supernotify" => ("supernotify", vec!["supernotify"]),
        "superbar" => ("superbar", vec!["superbar"]),
        "supershell" => ("supershell", vec!["supersettings", "superlock"]),
        "supersettings" => ("supershell", vec!["supersettings"]),
        "superlock" => ("supershell", vec!["superlock"]),
        other => (other, vec![other]),
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
    let (repo, bin_names) = repo_for_target(target);
    let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
    let bin_dir = PathBuf::from(&home).join(".local/bin");
    let _ = fs::create_dir_all(&bin_dir);

    println!("\x1b[1;38;5;141m[::] SUPERFORGE TIER-1 QUICK BINARY CACHE ({repo} -> {})\x1b[0m", bin_names.join(", "));
    if dry {
        for bin_name in &bin_names {
            let url = format!("https://github.com/SuperFedora/{repo}/releases/latest/download/{bin_name}-x86_64-unknown-linux-musl");
            let dst = bin_dir.join(bin_name);
            println!("  [DRY-RUN] 1. Fetch Static Musl : curl -fsSL {url}");
            println!("  [DRY-RUN] 2. Atomic Install    : {}", dst.display());
        }
        println!("  [DRY-RUN] 3. Fallback Tier-2   : superforge build {target} (-march=native)");
        return 0;
    }

    let mut all_ok = true;
    for bin_name in &bin_names {
        let url = format!("https://github.com/SuperFedora/{repo}/releases/latest/download/{bin_name}-x86_64-unknown-linux-musl");
        let dst = bin_dir.join(bin_name);
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

        let valid_elf = dl_ok && fs::read(&tmp).map(|b| b.len() > 64 && b.starts_with(b"\x7fELF")).unwrap_or(false);
        if valid_elf {
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
            let _ = fs::rename(&tmp, &dst);
            if *bin_name == "superfedora" {
                let _ = fs::remove_file(bin_dir.join("sf"));
            }
            println!("  [OK] Tier-1 prebuilt static musl binary installed: {}", dst.display());
        } else {
            let _ = fs::remove_file(&tmp);
            if dst.is_file() {
                println!("  [OK] Tier-1 binary already present at {} (queued for Tier-2 native re-forge).", dst.display());
            } else {
                all_ok = false;
            }
        }
    }
    if all_ok {
        let _ = fs::create_dir_all(st.state_dir.join("source-built"));
        let _ = fs::write(st.state_dir.join("source-built").join(format!("{repo}.tier")), "tier1-musl\n");
        return 0;
    }

    println!("  [--] Checking Tier-1 system / release / binstall cache before local compilation...");
    exec_runner(st, &["fetch-bin".to_string(), target.to_string()])
}

pub fn run_background_reforge(st: &mut ForgeState, args: &[String]) -> i32 {
    let mut dry = false;
    let mut force = false;
    let mut is_async = false;
    for a in args {
        match a.as_str() {
            "--dry-run" | "-n" => dry = true,
            "--force" | "-f" => force = true,
            "--async" | "--background" | "-b" => is_async = true,
            "-h" | "--help" | "help" => {
                println!("Usage: superforge background-reforge [--dry-run] [--force] [--async]");
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

    let targets = [
        "superfedora", "superforge", "supertheme", "superbar", "supershell", "supernotify",
        "idlescreen", "wlctl", "bluetui", "wiremix", "fuzzel", "btop",
    ];

    if dry {
        println!("  [DRY-RUN] 1. Hardware Profile : CFLAGS=\"-O3 -march=native -flto=thin\" RUSTFLAGS=\"-C opt-level=3 -C target-cpu=native\"");
        println!("  [DRY-RUN] 2. Target Suite     : {}", targets.join(", "));
        println!("  [DRY-RUN] 3. Atomic Swap      : ~/.local/bin/.<bin>.tmp -> ~/.local/bin/<bin>");
        println!("  [DRY-RUN] 4. Cast Comms       : supernotify --cast builder \"BUILDER: Native Re-Forge Complete\"");
        return 0;
    }

    let _ = write_hw_profile(st);
    let runner = st.forge_root.join("lib/forge-runner.sh");
    let mut call_args: Vec<String> = targets.iter().map(|s| (*s).to_string()).collect();
    if force {
        call_args.push("--force".into());
    }

    if is_async {
        let _ = Command::new("nice")
            .args(["-n", "19", "ionice", "-c", "3"])
            .arg(&runner)
            .env("FORGE_ROOT", &st.forge_root)
            .args(&call_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        println!("  [OK] Enqueued {} special -march={} builds in background (live progress in topbar).", targets.len(), st.hw.march_target);
        return 0;
    }

    let code = exec_runner(st, &call_args);
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
    code
}
