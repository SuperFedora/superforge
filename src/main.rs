// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

mod builder;
mod catalog;
mod tui;

use crate::builder::{exec_runner, print_status_and_charts, run_update_graphical, write_hw_profile};
use crate::catalog::ForgeState;
use crate::tui::run_tui;
use std::env;
use std::io::{self, IsTerminal};
use std::process;

fn print_help() {
    println!("SuperForge v0.1.0 — Native Rust Hardware-Tuned Source Builder & WebApp Engine");
    println!("Usage:");
    println!("  superforge                                  Launch TUI (or print status if non-TTY)");
    println!("  superforge tui                              Launch interactive 16-Bit Forge TUI");
    println!("  superforge status                           Show CPU/linker flags, presets & WebApps");
    println!("  superforge <preset|owner/repo>              Clone, tune (-march=native), build & install");
    println!("  superforge [webapp] <url> [name] [--isolated|--shared] Install chromeless Firefox WebApp");
    println!("  superforge remove <name|slug>               Uninstall a forged WebApp or binary");
    println!("  superforge update [--force] [--dry-run]     Update DNF RPMs, Flatpaks, Rustup, source builds & WebApps");
    println!("  superforge deps <target> [--install]        Check or install Fedora -devel dependencies");
    println!("  superforge profile                          Generate ~/.config/superfedora/hardware-flags.env");
    println!("  superforge all [--force]                    Compile core workstation suite for localhost CPU");
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
                println!("superforge 0.1.0 (SuperFedora/superforge — Rust 1.99 Native Engine)");
                return;
            }
            "tui" => {
                process::exit(run_tui());
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

    let cmd = args[0].as_str();
    match cmd {
        "status" | "--status" | "check" | "--check" | "list" | "presets" | "env" | "flags" => {
            print_status_and_charts(&st);
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
            for tgt in ["superfedora", "supertheme", "superforge", "supershell", "superagent", "wlctl", "bluetui", "wiremix", "fuzzel", "btop"] {
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
