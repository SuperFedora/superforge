// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

mod builder;
mod cache_tier;
mod catalog;
mod tui;
mod webapp;

use crate::builder::{exec_runner, print_status_and_charts, run_update_graphical, write_hw_profile};
use crate::cache_tier::{run_background_reforge, run_fetch_bin};
use crate::catalog::ForgeState;
use crate::tui::run_tui;
use crate::webapp::{run_webapp_exec, run_webapp_forge};
use std::env;
use std::io::{self, IsTerminal};
use std::process;

fn print_help() {
    println!("SuperForge v0.6.0 — Native Rust 2-Tier Binary Cache, Source Builder & WebApp Engine");
    println!("Usage:");
    println!("  superforge gui [tab]                        Launch standalone SuperForge GUI window");
    println!("  superforge tui                              Launch interactive 16-Bit Forge TUI");
    println!("  superforge status                           Show CPU/linker flags, stamps, presets & WebApps");
    println!("  superforge check-updates [--dry-run]        Compare local .stamp git SHAs vs remote HEAD");
    println!("  superforge fetch-bin <target> [--dry-run]   Tier-1 quick static musl binary fetch");
    println!("  superforge build --quick <target>           Fetch Tier-1 binary or fallback to source");
    println!("  superforge background-reforge [--dry-run]   Tier-2 AC-power -march=native background build");
    println!("  superforge <preset|owner/repo|recipe.toml>  Clone, tune (-march=native), build & install");
    println!("  superforge [webapp] <url> [name] [--isolated|--shared] Install chromeless Firefox WebApp");
    println!("  superforge remove <name|slug>               Uninstall a forged WebApp or binary");
    println!("  superforge update [--force] [--dry-run]     Update DNF RPMs, Flatpaks, Rustup, source builds & WebApps");
    println!("  superforge deps <target> [--install]        Check or install Fedora -devel dependencies");
    println!("  superforge profile                          Generate ~/.config/superfedora/hardware-flags.env");
    println!("  superforge all [--force]                    Compile core workstation suite for localhost CPU");
}

fn launch_gui(tab: &str) -> i32 {
    let t = if tab.is_empty() { "all" } else { tab };
    let ok = process::Command::new("qs")
        .args(["ipc", "call", "forge", "open", t])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { 0 } else { run_tui() }
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if matches!(args.first().map(|s| s.as_str()), Some("build" | "forge" | "install")) {
        args.remove(0);
        if args.is_empty() {
            args.push("status".into());
        } else if args.len() == 1 && (args[0] == "--force" || args[0] == "-f") {
            args = vec!["all".into(), "--force".into()];
        }
    }
    if let Some(first) = args.first().map(|s| s.as_str()) {
        match first {
            "-h" | "--help" | "help" => {
                print_help();
                return;
            }
            "-V" | "--version" | "version" => {
                println!("superforge 0.6.0 (SuperFedora/superforge — Rust 1.99 Native Engine)");
                return;
            }
            "gui" | "--gui" => {
                let tab = args.get(1).map(|s| s.as_str()).unwrap_or("all");
                process::exit(launch_gui(tab));
            }
            "tui" => {
                process::exit(run_tui());
            }
            "webapp-forge" => {
                process::exit(run_webapp_forge(&args[1..]));
            }
            "webapp-exec" | "webapp-launch" => {
                process::exit(run_webapp_exec(&args[1..]));
            }
            _ => {}
        }
    }

    let mut st = ForgeState::load();
    if args.is_empty() {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            process::exit(run_tui());
        } else {
            print_status_and_charts(&st);
            return;
        }
    }

    if args.iter().any(|a| a == "--quick") && args[0] != "fetch-bin" {
        let filtered: Vec<String> = args.into_iter().filter(|a| a != "--quick").collect();
        process::exit(run_fetch_bin(&mut st, &filtered));
    }

    let cmd = args[0].as_str();
    match cmd {
        "status" | "--status" | "check" | "--check" | "list" | "presets" | "env" | "flags" => {
            print_status_and_charts(&st);
        }
        "check-updates" | "check-update" | "outdated" => {
            let mut call_args = vec!["check-updates".to_string()];
            call_args.extend_from_slice(&args[1..]);
            process::exit(exec_runner(&mut st, &call_args));
        }
        "fetch-bin" | "fetch" | "quick" => {
            process::exit(run_fetch_bin(&mut st, &args[1..]));
        }
        "background-reforge" | "reforge" | "bg-reforge" => {
            process::exit(run_background_reforge(&mut st, &args[1..]));
        }
        "profile" | "init" => {
            let p = write_hw_profile(&st);
            println!(
                "[OK] Localhost hardware profile written to {} (-march={}, {} threads)",
                p.display(),
                st.hw.march_target,
                st.hw.cpu_jobs
            );
        }
        "all" => {
            let _ = write_hw_profile(&st);
            let force = args.iter().any(|a| a == "--force" || a == "-f");
            for tgt in ["superfedora", "supertheme", "superforge", "superbar", "supershell", "supernotify", "wlctl", "bluetui", "wiremix", "fuzzel", "btop"] {
                let mut call_args = vec![tgt.to_string()];
                if force {
                    call_args.push("--force".into());
                }
                let code = exec_runner(&mut st, &call_args);
                if code != 0 {
                    process::exit(code);
                }
            }
        }
        "update" | "upgrade" => {
            process::exit(run_update_graphical(&mut st, &args[1..]));
        }
        _ => {
            process::exit(exec_runner(&mut st, &args));
        }
    }
}
