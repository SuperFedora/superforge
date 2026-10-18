# SuperForge

[![Release](https://img.shields.io/github/v/release/SuperFedora/superforge?label=SuperForge)](https://github.com/SuperFedora/superforge/releases)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Native Rust hardware-tuned `-march=native` / LTO source builder and chromeless Firefox WebApp engine (`superforge`) for **[SuperFedora](https://superfedora.github.io/)**.

```bash
superforge                # Launch 16-bit Forge TUI (or print status & size chart)
superforge update         # 4-stage pipeline: DNF + Flatpak/Rustup + Git SHA LTO + WebApps
superforge <recipe|repo>  # Clone, stage user-sysroot headers, and compile with -march=native
superforge webapp <url>   # Install chromeless Firefox WebApp and pin to SuperDock
superforge remove <slug>  # Uninstall a forged binary or Firefox WebApp
```

* **Catalog:** `recipes.toml` and `packages/forge.txt` define native build targets (`superfedora`, `supertheme`, `superforge`, `superbar`, `supershell`, `supernotify`, `hyprland`, `quickshell`, `idlescreen`, `btop`, `ripgrep`, `fastfetch`, `fuzzel`, `wiremix`, `wlctl`, `bluetui`, `openrgb`).

**Full documentation & architecture guide: [superfedora.github.io](https://superfedora.github.io/)**

Licensed under the Apache License, Version 2.0.
