# SuperFedora: House Laws & Agent Engineering Standards (v1)

This document is the **single canonical source of truth** for all code, architecture, system integration standards, and automated pipelines across the `SuperFedora` organization. Every human contributor and AI agent must strictly follow these rules without exception.

> **Synchronization Invariant**: This document is synchronized across all `SuperFedora` repositories. The canonical upstream copy lives at [`SuperFedora/.github/AGENTS.md`](https://github.com/SuperFedora/.github/blob/main/AGENTS.md). Automated CI gates enforce that every repository's `AGENTS.md` remains bit-for-bit identical to upstream.

**Precedence:** Where this file and CI disagree, this file wins. A rule not enforced anywhere is a bug in this file, not a suggestion to ignore.

**Rule Markers:**
- `[GATE:<tool>]` — Machine-enforced by automated linters, compilers, or CI checks. Must pass before merging or release.
- `[POLICY]` — Enforced during code review and multi-engine audits. Governs architecture, naming, taste, safety, and ergonomics.

---

## 1. The Page Rule (Shell, QML & Layout Boundaries)

A **page** is one committed shell script, QML component, or configuration file. Every page holds one idea, fits in one head, and carries its own weight.

- `[GATE:lint]` **16–256 Lines**: Every committed shell script (`.sh`), helper, and module must be between **16 and 256 lines** (blank lines and comments count).
- `[GATE:lint]` **Directory Density (<= 12 files)**: At most 12 functional files per directory. Crowded directories split into functional domain subdirectories.
- `[GATE:lint]` **Banned Drawer Filenames**:
  `util.sh`, `utils.sh`, `helper.sh`, `helpers.sh`, `common.sh`, `misc.sh`, `shared.sh`, `base.sh`.
  Name the specific capability (`check.sh`, `install-dotfiles.sh`, `setup-btrfs-snapper.sh`, `core.sh`).
- `[POLICY]` **Module Ordering & Naming**:
  Installer modules lead with a two-digit execution order: `00-preflight.sh`, `10-repos.sh`, `20-base.sh`, `30-session.sh`, `40-desktop.sh`, `50-apps.sh`, `60-idlescreen.sh`, `70-dev.sh`, `80-ai.sh`, `90-dotfiles.sh`.
  Action pages lead with an active verb (`setup-btrfs-snapper.sh`, `gen-keybindings-doc.py`).
- `[POLICY]` **Never Pad**: Do not add dummy comments or blank lines to clear 16 lines. Fold small helpers into their single caller or nearest sibling.

---

## 2. Workstation Architecture & System Contracts

- `[GATE:ci]` **100% Idempotency Invariant**:
  Every installer module and CLI command must be safe to re-run indefinitely on a live machine. Running `sf install` 10 times consecutively must yield identical state, zero duplications, zero spurious errors, and exit code 0.
- `[GATE:ci]` **Detection Before Action**:
  Every module must query system state first (`has_pkg`, `has_copr`, `has_cmd`, `has_line`). If an item is already present, report `[OK]` and perform zero mutating operations.
- `[POLICY]` **Dual CLI Ergonomics (`sf` and `superfedora`)**:
  The orchestrator CLI must always provide `sf` as the primary, fast 2-letter command alongside `superfedora`. Both binaries/symlinks must be installed to `~/.local/bin/` by the web bootstrap installer.
- `[POLICY]` **Zero-Symlink Dotfiles Invariant**:
  Managed user configurations in `~/.config/` are deployed as direct files, never fragile relative symlinks. Any pre-existing user configuration must be safely backed up with timestamps to `~/.config/superfedora/backups/`.
- `[POLICY]` **Native systemd Citizenship**:
  Graphical desktop components bind to `hyprland-session.target`. Daemons run through `uwsm` or systemd user units, cleanly responding to `SIGTERM` with standardized exit codes (`0`, `69`, `78`).
- `[POLICY]` **Btrfs Snapper Rollback Guarantee**:
  Module `00-preflight` must capture an immutable Snapper baseline snapshot (#1) labeled `superfedora-preinstall-baseline` before any package or system file changes. Rollback is cleanly triggered via `sf rollback`.

---

## 3. Style, Licensing & Zero-Emoji Discipline

- `[GATE:lint]` **Strict Apache 2.0 License Invariant**:
  All repositories, documentation, scripts, and web assets across `@SuperFedora` are licensed under the Apache License, Version 2.0. Every repository root must contain the canonical `LICENSE` file.
- `[GATE:lint]` **Strict Zero-Emoji Invariant**:
  Emojis and unicode pictographs are strictly prohibited across all code, scripts, HTML/CSS, markdown documentation, commit messages, and release notes. Use clean retro ASCII indicators (`[OK]`, `[--]`, `[FAIL]`, `[+]`, `*`) or structured text.
- `[POLICY]` **Lightweight Repository READMEs**:
  Repository README files must remain concise (under 25 lines) and serve as clear entrypoints directing users to the official web portal (`https://superfedora.github.io`) for complete documentation.
- `[POLICY]` **16-Bit Super Aesthetics**:
  Website assets, avatars, and wallpapers strictly adhere to retro 16-bit SNES / Tokyo Night / Super Metroid aesthetics with pixel art curation.

---

## 4. Verification Ladder

Order matters. Each step must pass completely before releasing or publishing:

1. `bash -n bin/superfedora lib/*.sh modules/*.sh install.sh` — Zero bash syntax errors.
2. `shellcheck -x -e SC1091 bin/superfedora lib/*.sh modules/*.sh install.sh` — Zero ShellCheck warnings.
3. `python3 scripts/gen-keybindings-doc.py --check` — Keybindings docs synchronized with `bindings.conf`.
4. `bash tests/test_installer.sh` — Web installer unit tests pass.
5. Container CI — Headless installation on clean Fedora container passes and achieves 100% idempotency.

---

## 5. Multi-Engine Audit Protocol (Pre-Release)

Advisory tiered audit run before tagging releases, budgeted by token capacity:
1. **`mcode`** (Abundant tokens): Full codebase scan for contract mismatches, idempotency leaks, and CI gates.
   `mcode exec --permission full "<prompt>"`
2. **`vibe`** (Moderate tokens): Adversarial diff inspection for subtle regressions, safety leaks, and edge cases.
   `vibe -p "<prompt>" --trust --auto-approve`
3. **`grok`** (Strict quota): High-value referee and final sanity check on critical release diffs.
   `grok -p "<prompt>" --always-approve`

**Rule:** External audits are strictly advisory; all findings must be verified against source code. External LLMs never gate CI—the §4 Verification Ladder is the sole canonical authority.
