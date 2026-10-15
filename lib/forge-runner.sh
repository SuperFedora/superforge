#!/usr/bin/env bash
# Copyright 2026 SuperFedora Contributors
# SPDX-License-Identifier: Apache-2.0
# ==============================================================================
# forge-runner.sh — SuperForge Hardware-Tuned Source Builder & WebApp Runner
# ==============================================================================
# Usage:
#   superforge status                           Show CPU/linker flags, presets & WebApps
#   superforge <preset|owner/repo>              Clone, tune (-march=native), build & install
#   superforge [webapp] <url> [name] [--isolated|--shared] Install chromeless Firefox WebApp
#   superforge remove <name|slug>               Uninstall a forged WebApp or binary
#   superforge update [--force]                 Update DNF RPMs, Flatpaks, Rustup, source builds & WebApps
#   superforge deps <target> [--install]        Check or install Fedora -devel dependencies
# ==============================================================================
set -euo pipefail

FORGE_ROOT="$(cd "$(dirname "$(realpath "${BASH_SOURCE[0]}")")/.." && pwd)"
SF_ROOT="${SF_ROOT:-$HOME/Projects/superfedora/superfedora}"
STATE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/superfedora"; STAMP_DIR="$STATE_DIR/source-built"
FORGE_JSON="$STATE_DIR/forge.json"; LIVE_JSON="$STATE_DIR/forge-live.json"; BIN_DIR="$HOME/.local/bin"
WEBAPP_DIR="$HOME/.local/share/superfedora/webapps"; SYSROOT="$HOME/.local/sysroot/usr"
TMP_BUILD="${XDG_CACHE_HOME:-$HOME/.cache}/superfedora/forge-build"; FORGE_LOG="$TMP_BUILD/forge.log"
mkdir -p "$TMP_BUILD" "$STATE_DIR" "$BIN_DIR"; FORGE_ACTIVE_TARGET=""; CURRENT_TMP=""
export TMPDIR="$TMP_BUILD" PATH="$BIN_DIR:$HOME/.cargo/bin:$SYSROOT/bin:$SYSROOT/libexec/qt6:/usr/bin:/bin:$PATH"
[[ -d "$SYSROOT" ]] && export LD_LIBRARY_PATH="$SYSROOT/lib64:${LD_LIBRARY_PATH:-}" LIBRARY_PATH="$SYSROOT/lib64:$HOME/.local/usr/lib64:$HOME/.local/lib:/usr/lib64" CPATH="$SYSROOT/include:$HOME/.local/usr/include" PKG_CONFIG_PATH="$SYSROOT/lib64/pkgconfig:$SYSROOT/share/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig" CMAKE_PREFIX_PATH="$SYSROOT;/usr"

set_live() {
    [[ "$1" == "1" ]] && FORGE_ACTIVE_TARGET="$3" || FORGE_ACTIVE_TARGET=""
    python3 -c 'import json,sys,time; p,a,st,t,pct,s=sys.argv[1:7]; json.dump({"active":a=="1","state":st,"target":t,"pct":int(pct),"step":s,"ts":int(time.time())},open(p,"w"))' "$LIVE_JSON" "$1" "$2" "$3" "$4" "$5" 2>/dev/null || true
    echo "$5" | tee -a "$FORGE_LOG"; command -v qs >/dev/null 2>&1 && { qs ipc call status reloadForge >/dev/null 2>&1 || qs ipc call status refresh >/dev/null 2>&1 || true; }
    [[ "$1" == "0" && "$2" == "ok" && "${IN_UPDATE:-0}" -eq 0 && "$5" != *already* && "$5" != *Removed* ]] && { command -v supernotify >/dev/null 2>&1 && supernotify --banner update >/dev/null 2>&1 || true; }
}
trap '[[ -n "${CURRENT_TMP:-}" ]] && rm -rf "$CURRENT_TMP"; [[ -n "${FORGE_ACTIVE_TARGET:-}" ]] && set_live 0 error "$FORGE_ACTIVE_TARGET" 100 "[FAIL] Forge interrupted or failed (see forge.log)" || true' EXIT

detect_hw() {
    CPU_ARCH="$(uname -m)"; CPU_MODEL="$(awk -F': ' '/^model name/{print $2; exit}' /proc/cpuinfo 2>/dev/null || echo "$CPU_ARCH")"; CPU_JOBS="$(nproc 2>/dev/null || echo 4)"; MARCH_TARGET=""
    command -v rustc >/dev/null 2>&1 && MARCH_TARGET="$(rustc --print target-cpus 2>/dev/null | sed -n 's/.*currently \([^)]*\)).*/\1/p' | head -1)"
    [[ -z "$MARCH_TARGET" && -x /usr/bin/gcc ]] && MARCH_TARGET="$(/usr/bin/gcc -march=native -Q --help=target 2>/dev/null | awk '/^\s*-march=/{print $2; exit}')"
    MARCH_TARGET="${MARCH_TARGET:-native}"
    ISA_FLAGS="$(grep -m1 '^flags' /proc/cpuinfo 2>/dev/null | grep -oE '\b(avx512f|avx2|avx|fma|bmi2|aes|vaes|sha_ni|sse4_2)\b' | sort -u | tr '\n' ' ' | sed 's/ $//' || echo "standard")"; [[ -n "$ISA_FLAGS" ]] || ISA_FLAGS="standard"
    LINKER="bfd"; LD_FLAG=""; RUST_LD=""
    if command -v mold >/dev/null 2>&1; then LINKER="mold"; LD_FLAG="-fuse-ld=mold"; RUST_LD="-C link-arg=-fuse-ld=mold"
    elif command -v ld.lld >/dev/null 2>&1; then LINKER="lld"; LD_FLAG="-fuse-ld=lld"; RUST_LD="-C link-arg=-fuse-ld=lld"; fi
    GPU_INFO="$(lspci 2>/dev/null | awk -F': ' '/VGA|3D|Display/{print $2; exit}' || echo "Wayland/DRM")"
    HOST_CFLAGS="-O3 -pipe -march=native -mtune=native -flto=auto -ffat-lto-objects -fno-plt"; HOST_CXXFLAGS="$HOST_CFLAGS"
    HOST_LDFLAGS="-Wl,-O1,--sort-common,--as-needed,-z,relro,-z,now,-rpath,$SYSROOT/lib64 -flto=auto ${LD_FLAG}"
    HOST_RUSTFLAGS="-C opt-level=3 -C target-cpu=native -C codegen-units=1 -C strip=symbols ${RUST_LD}"
}

ensure_pkgs() {
    local -a missing=(); local p l f
    for p in "$@"; do [[ "$p" =~ ^(gcc|gcc-c\+\+|make|cmake|ninja-build)$ ]] && command -v "${p%-build}" >/dev/null 2>&1 && continue; rpm -q --quiet "$p" 2>/dev/null || [[ -f "$HOME/.local/pkg_cache/.ok-$p" ]] || missing+=("$p"); done
    [[ ${#missing[@]} -eq 0 ]] && return 0
    if [[ $EUID -eq 0 ]]; then dnf install -y "${missing[@]}"
    elif [[ "${INSTALL_DEPS:-0}" -eq 1 ]]; then sudo dnf install -y "${missing[@]}"
    else
        echo "[+] Staging build headers into user sysroot: ${missing[*]}" | tee -a "$FORGE_LOG"
        mkdir -p "$HOME/.local/pkg_cache" "$SYSROOT/lib64" "$SYSROOT/bin" "$SYSROOT/libexec/qt6"
        dnf download --resolve --arch="$(uname -m)" --arch=noarch --destdir="$HOME/.local/pkg_cache" "${missing[@]}" >/dev/null 2>&1 || true
        for p in "$HOME/.local/pkg_cache"/*.rpm; do [[ -f "$p" && ! -f "$HOME/.local/pkg_cache/.extracted-$(basename "$p")" ]] && (cd "$HOME/.local/sysroot" && rpm2cpio "$p" 2>/dev/null | cpio -idmu --quiet 2>/dev/null || true) && touch "$HOME/.local/pkg_cache/.extracted-$(basename "$p")"; done
        for f in /usr/lib64/lib*.so*; do [[ -e "$SYSROOT/lib64/${f##*/}" ]] || ln -s "$f" "$SYSROOT/lib64/${f##*/}" 2>/dev/null || true; done
        for f in /usr/libexec/qt6/*; do [[ -e "$SYSROOT/libexec/qt6/${f##*/}" ]] || ln -s "$f" "$SYSROOT/libexec/qt6/${f##*/}" 2>/dev/null || true; done
        mkdir -p "$SYSROOT/lib64/qt6" && cp -rs /usr/lib64/qt6/* "$SYSROOT/lib64/qt6/" 2>/dev/null || true
        for f in /usr/bin/*; do [[ -e "$SYSROOT/bin/${f##*/}" ]] || ln -s "$f" "$SYSROOT/bin/${f##*/}" 2>/dev/null || true; done
        for l in "$SYSROOT/lib64"/lib*.so*; do [[ -L "$l" && ! -e "$l" && -e "/usr/lib64/$(readlink "$l")" ]] && ln -sf "/usr/lib64/$(readlink "$l")" "$l" || true; done
        sed -i "s|$HOME/.local/sysroot||g; s|/usr/lib64/libc_nonshared.a|$SYSROOT/lib64/libc_nonshared.a|g; s|/usr/lib64/libmvec_nonshared.a|$SYSROOT/lib64/libmvec_nonshared.a|g" "$SYSROOT/lib64/libc.so" "$SYSROOT/lib64/libm.so" 2>/dev/null || true
        sed -i "s|$HOME/.local/sysroot||g; s|^prefix=/usr$|prefix=$SYSROOT|g; s|/usr/include|$SYSROOT/include|g; s|/usr/lib64|$SYSROOT/lib64|g" "$SYSROOT"/lib64/pkgconfig/*.pc "$SYSROOT"/share/pkgconfig/*.pc 2>/dev/null || true
        find "$SYSROOT/lib64/cmake" -name "*.cmake" -exec sed -i "s|$HOME/.local/sysroot||g; s|/usr/include|$SYSROOT/include|g; s|/usr/lib64/qt6|$SYSROOT/lib64/qt6|g; s|/usr/libexec/qt6|$SYSROOT/libexec/qt6|g" {} + 2>/dev/null || true
        sed -i 's/!isnormal(val)/!__builtin_isnormal(val)/g; s/isinf(val)/__builtin_isinf(val)/g; s/signbit(val)/__builtin_signbit(val)/g' "$SYSROOT/include/spa-0.2/spa/utils/json-core.h" 2>/dev/null || true
        [[ -x "$SYSROOT/bin/qmake6" ]] && printf '[Paths]\nPrefix = %s\nArchData = lib64/qt6\nData = share/qt6\nHeaders = include/qt6\nLibraries = lib64\nLibraryExecutables = lib64/qt6/libexec\nBinaries = lib64/qt6/bin\nHostPrefix = %s\nHostData = lib64/qt6\nHostBinaries = lib64/qt6/bin\nHostLibraryExecutables = lib64/qt6/libexec\nHostLibraries = lib64\n' "$SYSROOT" "$SYSROOT" > "$SYSROOT/bin/qt.conf"
        for p in "${missing[@]}"; do touch "$HOME/.local/pkg_cache/.ok-$p"; done
    fi
}

preset_deps() {
    case "$1" in
        superfedora|supertheme|superforge|supershell|superagent|ripgrep|rg|fd|zoxide) echo "gcc make" ;;
        idlescreen)  echo "gcc make pkgconf-pkg-config wayland-devel libxkbcommon-devel dbus-devel" ;;
        fastfetch)   echo "gcc gcc-c++ cmake ninja-build pkgconf-pkg-config pciutils-devel wayland-devel libdrm-devel glib2-devel" ;;
        btop)        echo "gcc-c++ make" ;;
        wlctl|bluetui) echo "gcc make pkgconf-pkg-config dbus-devel" ;;
        wiremix)     echo "gcc clang make pkgconf-pkg-config pipewire-devel alsa-lib-devel" ;;
        fuzzel)      echo "gcc meson ninja-build pkgconf-pkg-config wayland-devel wayland-protocols-devel libxkbcommon-devel pixman-devel cairo-devel pango-devel fcft-devel tllist-devel fontconfig-devel libpng-devel librsvg2-devel scdoc" ;;
        openrgb)     echo "gcc-c++ make pkgconf-pkg-config qt6-qtbase-devel qt6-linguist hidapi-devel libusb1-devel mbedtls-devel" ;;
        hyprland)    echo "gcc-c++ cmake meson ninja-build pkgconf-pkg-config wayland-devel wayland-protocols-devel cairo-devel pango-devel pixman-devel libdrm-devel mesa-libEGL-devel mesa-libGLES-devel mesa-libgbm-devel libinput-devel libxkbcommon-devel libuuid-devel tomlplusplus-devel hyprlang-devel hyprcursor-devel hyprutils-devel hyprwayland-scanner-devel aquamarine-devel hyprgraphics-devel hyprwire-devel glaze-devel re2-devel muParser-devel libdisplay-info-devel libliftoff-devel libseat-devel hwdata-devel xcb-util-devel xcb-util-wm-devel xcb-util-errors-devel xcb-util-renderutil-devel xcb-util-keysyms-devel xcb-util-image-devel xorg-x11-server-Xwayland-devel libXcursor-devel systemd-devel lua-devel readline-devel" ;;
        quickshell)  echo "gcc-c++ cmake ninja-build pkgconf-pkg-config spirv-tools qt6-qtbase-devel qt6-qtbase-private-devel qt6-qtdeclarative-devel qt6-qtwayland-devel qt6-qtsvg-devel qt6-qtshadertools-devel wayland-devel wayland-protocols-devel libdrm-devel mesa-libgbm-devel pipewire-devel pam-devel cli11-devel jemalloc-devel polkit-devel glib2-devel breakpad-devel" ;;
        *)           echo "gcc gcc-c++ make cmake" ;;
    esac
}

record_forge() {
    local name="$1" repo="$2" bin="$3" sys="$4" sha="${5:-}" ver; mkdir -p "$STATE_DIR" "$STAMP_DIR"
    ver="$("$BIN_DIR/$bin" --version 2>/dev/null | head -1 || echo "native-$MARCH_TARGET")"
    printf 'component=%s\nrepo=%s\narch=%s\ncommit=%s\nbuilt_at=%s\ncflags=%s\nrustflags=%s\n' "$name" "$repo" "$MARCH_TARGET" "$sha" "$(date -u +%FT%TZ)" "$HOST_CFLAGS" "$HOST_RUSTFLAGS" > "$STAMP_DIR/$name.stamp"
    python3 -c 'import datetime,json,os,re,sys; p,name,repo,bpath,bsys,arch,lnk,ver,sha=sys.argv[1:10]; ver=re.sub(r"\x1b\[[0-9;]*m","",ver).strip(); d=json.load(open(p)) if os.path.exists(p) else {}; d.setdefault("targets",{})[name]={"type":"source","repo":repo,"binary":bpath,"build_system":bsys,"march":arch,"linker":lnk,"version":ver,"commit":sha,"built_at":datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}; json.dump(d,open(p,"w"),indent=2)' "$FORGE_JSON" "$name" "$repo" "$BIN_DIR/$bin" "$sys" "$MARCH_TARGET" "$LINKER" "$ver" "$sha"
}

install_bin() {
    local src="$1" dst_name="${2:-$(basename "$1")}"; mkdir -p "$BIN_DIR"
    install -m 755 "$src" "$BIN_DIR/.${dst_name}.tmp.$$" && mv -f "$BIN_DIR/.${dst_name}.tmp.$$" "$BIN_DIR/$dst_name"
    [[ "$dst_name" == "superfedora" && -d "$SF_ROOT/bin" ]] && { install -m 755 "$src" "$SF_ROOT/bin/.sf.tmp.$$" && mv -f "$SF_ROOT/bin/.sf.tmp.$$" "$SF_ROOT/bin/superfedora"; ln -sf "$BIN_DIR/superfedora" "$BIN_DIR/sf"; }
    echo "[OK] Installed $BIN_DIR/$dst_name (-march=$MARCH_TARGET, linker=$LINKER)" | tee -a "$FORGE_LOG"
}

forge_webapp() {
    local raw_url="$1" custom_name="${2:-}" prof_mode="${3:-shared}" dry="${4:-0}" res app_id title
    [[ "$raw_url" == *"|"* && -z "$custom_name" ]] && { custom_name="${raw_url#*|}" && raw_url="${raw_url%%|*}"; }
    [[ "$raw_url" =~ ^https?:// ]] || raw_url="https://$raw_url"
    [[ "$dry" -eq 1 ]] && { printf '[DRY-RUN] Would forge Firefox WebApp for %s (profile=%s)\n' "$raw_url" "$prof_mode"; return 0; }
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && : > "$FORGE_LOG"
    set_live 1 running "${custom_name:-$raw_url}" 20 "[1/4] Probing webpage metadata & high-res icon for $raw_url..."
    res="$(python3 "$FORGE_ROOT/scripts/forge-webapp.py" "$raw_url" "$custom_name" "$prof_mode" "$WEBAPP_DIR" "$BIN_DIR" "$FORGE_JSON" "$LIVE_JSON")"
    IFS='|' read -r app_id title <<< "$res"
    set_live 1 running "$title" 92 "[4/4] Registering desktop entry & pinning to SuperDock..."
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
    [[ "${NO_PIN:-0}" -eq 0 ]] && { command -v superdock >/dev/null 2>&1 && superdock pin "$app_id" >/dev/null 2>&1 || true; }
    set_live 0 ok "$title" 100 "[OK] Installed WebApp '$title' ($prof_mode Firefox profile)"
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && { command -v supertheme >/dev/null 2>&1 && supertheme sfx play confirm >/dev/null 2>&1 || true; }
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && command -v notify-send >/dev/null 2>&1 && notify-send -a "SF FORGE" "WebApp Ready: $title" "Installed chromeless Firefox WebApp ($prof_mode profile) and pinned to SuperDock." || true
}

forge_generic_dir() {
    local dir="$1" name="$2" repo="$3" bin_hint="${4:-$2}" extra_cargo="${5:-}" cc_bin="gcc" cxx_bin="g++" found sha
    sha="$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)"
    [[ -x "$BIN_DIR/gcc" ]] && cc_bin="$BIN_DIR/gcc" || { [[ -x /usr/bin/gcc ]] && cc_bin="/usr/bin/gcc"; }
    [[ -x "$BIN_DIR/g++" ]] && cxx_bin="$BIN_DIR/g++" || { [[ -x /usr/bin/g++ ]] && cxx_bin="/usr/bin/g++"; }
    set_live 1 running "$name" 65 "[3/4] Compiling $name (-O3 -march=$MARCH_TARGET, $CPU_JOBS threads)..."
    if [[ -f "$dir/Cargo.toml" ]]; then
        local rtarget="${CPU_ARCH}-unknown-linux-gnu"
        # shellcheck disable=SC2086
        (cd "$dir" && CC="$cc_bin" CXX="$cxx_bin" CFLAGS="$HOST_CFLAGS" CXXFLAGS="$HOST_CXXFLAGS" RUSTFLAGS="$HOST_RUSTFLAGS" CARGO_BUILD_JOBS="$CPU_JOBS" cargo build --release --target "$rtarget" $extra_cargo 2>&1 | tee -a "$FORGE_LOG")
        found="$dir/target/$rtarget/release/$bin_hint"; [[ -x "$found" ]] || found="$dir/target/release/$bin_hint"
        [[ -x "$found" ]] || found="$(find "$dir/target/$rtarget/release" "$dir/target/release" -maxdepth 1 -type f -executable ! -name '*.so' ! -name '*.d' 2>/dev/null | head -1)"
        install_bin "$found" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "cargo" "$sha"
        [[ "$name" == "idlescreen" && -x "${found%/*}/idlescreen-extras" ]] && install_bin "${found%/*}/idlescreen-extras" "idlescreen-extras"
        if [[ "$name" == "supershell" ]]; then for sb in superdock superbar supersettings supernotify superlock; do [[ -x "${found%/*}/$sb" ]] && install_bin "${found%/*}/$sb" "$sb"; done; fi
    elif [[ -f "$dir/CMakeLists.txt" ]]; then
        local gen="-G Unix Makefiles" extra_cmake="-DBUILD_TESTING=OFF -DCMAKE_PREFIX_PATH=$SYSROOT;/usr"; command -v ninja >/dev/null 2>&1 && gen="-G Ninja"
        [[ "$name" == "quickshell" ]] && extra_cmake="$extra_cmake -DCRASH_HANDLER=OFF -DCRASH_REPORTER=OFF -DDISTRIBUTOR=SuperFedora"
        [[ "$name" == "hyprland" ]] && extra_cmake="$extra_cmake -DNO_TESTS=ON -DNO_HYPRPM=OFF"
        # shellcheck disable=SC2086
        CC="$cc_bin" CXX="$cxx_bin" CFLAGS="$HOST_CFLAGS -ffat-lto-objects" CXXFLAGS="$HOST_CXXFLAGS -ffat-lto-objects" LDFLAGS="$HOST_LDFLAGS -L$SYSROOT/lib64" cmake -B "$dir/build" -S "$dir" $gen -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOME/.local" -Wno-dev $extra_cmake 2>&1 | tee -a "$FORGE_LOG"
        cmake --build "$dir/build" -j "$CPU_JOBS" 2>&1 | tee -a "$FORGE_LOG"
        found="$(find "$dir/build" -type f -executable -name "$bin_hint" | head -1)"; [[ -n "$found" ]] || found="$(find "$dir/build" -maxdepth 3 -type f -executable ! -name '*.so*' ! -name 'CMake*' | head -1)"
        install_bin "$found" "$bin_hint"; [[ "$name" == "quickshell" ]] && install_bin "$found" "qs"
        [[ "$name" == "hyprland" ]] && { local hctl; hctl="$(find "$dir/build" -type f -executable -name "hyprctl" | head -1)"; [[ -n "$hctl" ]] && install_bin "$hctl" "hyprctl"; }
        record_forge "$name" "$repo" "$bin_hint" "cmake" "$sha"
    elif [[ -f "$dir/meson.build" ]]; then
        CC="$cc_bin" CXX="$cxx_bin" CFLAGS="$HOST_CFLAGS" CXXFLAGS="$HOST_CXXFLAGS" LDFLAGS="$HOST_LDFLAGS" meson setup "$dir/build" "$dir" --prefix="$HOME/.local" -Dbuildtype=release -Db_lto=true 2>&1 | tee -a "$FORGE_LOG"
        ninja -C "$dir/build" -j "$CPU_JOBS" 2>&1 | tee -a "$FORGE_LOG"; found="$(find "$dir/build" -type f -executable -name "$bin_hint" | head -1)"; install_bin "$found" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "meson" "$sha"
    elif compgen -G "$dir/*.pro" >/dev/null; then
        local qm="$SYSROOT/bin/qmake6"; [[ -x "$qm" ]] || qm="$(command -v qmake6 || command -v qmake-qt5 || command -v qmake)"
        (cd "$dir" && "$qm" INCLUDEPATH+="$SYSROOT/include $SYSROOT/include/hidapi $SYSROOT/include/libusb-1.0" QMAKE_CC="$cc_bin" QMAKE_CXX="$cxx_bin" QMAKE_LINK="$cxx_bin" QMAKE_CFLAGS_RELEASE="$HOST_CFLAGS" QMAKE_CXXFLAGS_RELEASE="$HOST_CXXFLAGS" QMAKE_LFLAGS_RELEASE="$HOST_LDFLAGS -L$SYSROOT/lib64" && make -j "$CPU_JOBS" 2>&1 | tee -a "$FORGE_LOG")
        found="$(find "$dir" -maxdepth 2 -type f -executable -iname "$bin_hint" | head -1)"; install_bin "$found" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "qmake" "$sha"
    elif [[ -f "$dir/Makefile" || -f "$dir/GNUmakefile" ]]; then
        CC="$cc_bin" CXX="$cxx_bin" make -C "$dir" OPTFLAGS="-O3 -march=native -mtune=native -flto=auto" -j "$CPU_JOBS" 2>&1 | tee -a "$FORGE_LOG"
        found="$(find "$dir" -maxdepth 2 -type f -executable -name "$bin_hint" | head -1)"; install_bin "$found" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "make" "$sha"
    elif [[ -f "$dir/go.mod" ]]; then (cd "$dir" && CGO_CFLAGS="$HOST_CFLAGS" go build -ldflags="-s -w" -o "$bin_hint" ./...); install_bin "$dir/$bin_hint" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "go" "$sha"
    elif [[ -f "$dir/build.zig" ]]; then (cd "$dir" && zig build -Doptimize=ReleaseFast -Dcpu=native); install_bin "$dir/zig-out/bin/$bin_hint" "$bin_hint"; record_forge "$name" "$repo" "$bin_hint" "zig" "$sha"
    else set_live 0 error "$name" 100 "[FAIL] No supported build manifest in $dir"; return 1; fi
    set_live 0 ok "$name" 100 "[OK] Forged $name (-march=$MARCH_TARGET, linker=$LINKER)"
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && { command -v supertheme >/dev/null 2>&1 && supertheme sfx play confirm >/dev/null 2>&1 || true; }
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && command -v notify-send >/dev/null 2>&1 && notify-send -a "SF FORGE" "Forged $name" "Compiled with -O3 -march=$MARCH_TARGET and installed to $BIN_DIR/$bin_hint." || true
}

forge_target() {
    local raw="${1%/}" force="${2:-0}" dry="${3:-0}" custom_name="${4:-}" prof_mode="${5:-shared}"
    if [[ "$raw" =~ ^https?:// ]] && ! [[ "$raw" =~ ^https?://(www\.)?(github\.com|gitlab\.com|codeberg\.org)/[^/|]+/[^/|]+$ ]]; then forge_webapp "$raw" "$custom_name" "$prof_mode" "$dry"; return $?
    elif [[ "$raw" == *.* && "$raw" != */* ]]; then forge_webapp "https://$raw" "$custom_name" "$prof_mode" "$dry"; return $?; fi
    raw="${raw%.git}"; raw="${raw#https://github.com/}"; raw="${raw#http://github.com/}"; raw="${raw#github.com/}"; detect_hw
    local name="$raw" repo="" bin="$raw" cargo_args="" local_src="" git_branch=""
    case "${raw,,}" in
        superfedora|sf|superfedora/superfedora) name="superfedora"; repo="https://github.com/SuperFedora/superfedora.git"; bin="superfedora"; [[ -d "$HOME/Projects/superfedora/superfedora" ]] && local_src="$HOME/Projects/superfedora/superfedora" ;;
        supertheme|superfedora/supertheme) name="supertheme"; repo="https://github.com/SuperFedora/supertheme.git"; bin="supertheme"; [[ -d "$HOME/Projects/superfedora/supertheme" ]] && local_src="$HOME/Projects/superfedora/supertheme" ;;
        superforge|superfedora/superforge) name="superforge"; repo="https://github.com/SuperFedora/superforge.git"; bin="superforge"; local_src="$FORGE_ROOT" ;;
        supershell|superfedora/supershell) name="supershell"; repo="https://github.com/SuperFedora/supershell.git"; bin="superdock"; cargo_args="--workspace"; [[ -d "$HOME/Projects/superfedora/supershell" ]] && local_src="$HOME/Projects/superfedora/supershell" ;;
        superagent|superfedora/superagent) name="superagent"; repo="https://github.com/SuperFedora/superagent.git"; bin="superagent"; [[ -d "$HOME/Projects/superfedora/superagent" ]] && local_src="$HOME/Projects/superfedora/superagent" ;;
        ripgrep|rg|burntsushi/ripgrep) name="ripgrep"; repo="https://github.com/BurntSushi/ripgrep.git"; bin="rg" ;;
        fastfetch|fastfetch-cli/fastfetch) name="fastfetch"; repo="https://github.com/fastfetch-cli/fastfetch.git"; bin="fastfetch" ;;
        idlescreen|idlescreen/idlescreen) name="idlescreen"; repo="https://github.com/idlescreen/idlescreen.git"; bin="idlescreen"; cargo_args="--workspace"; [[ -d "$HOME/Projects/idlescreen/idlescreen" ]] && local_src="$HOME/Projects/idlescreen/idlescreen" ;;
        fd|sharkdp/fd) name="fd"; repo="https://github.com/sharkdp/fd.git"; bin="fd" ;;
        zoxide|ajeetdsouza/zoxide) name="zoxide"; repo="https://github.com/ajeetdsouza/zoxide.git"; bin="zoxide" ;;
        btop|aristocratos/btop) name="btop"; repo="https://github.com/aristocratos/btop.git"; bin="btop" ;;
        wlctl|aashish-thapa/wlctl) name="wlctl"; repo="https://github.com/aashish-thapa/wlctl.git"; bin="wlctl" ;;
        bluetui|pythops/bluetui) name="bluetui"; repo="https://github.com/pythops/bluetui.git"; bin="bluetui" ;;
        wiremix|tsowell/wiremix) name="wiremix"; repo="https://github.com/tsowell/wiremix.git"; bin="wiremix" ;;
        fuzzel|dnkl/fuzzel) name="fuzzel"; repo="https://codeberg.org/dnkl/fuzzel.git"; bin="fuzzel" ;;
        openrgb|calcprogrammer1/openrgb) name="openrgb"; repo="https://gitlab.com/CalcProgrammer1/OpenRGB.git"; bin="openrgb" ;;
        hyprland|hyprwm/hyprland) name="hyprland"; repo="https://github.com/hyprwm/Hyprland.git"; bin="Hyprland"; git_branch="v$(rpm -q --qf '%{VERSION}' hyprland 2>/dev/null || echo 0.56.2)" ;;
        quickshell|outfoxxed/quickshell|quickshell-mirror/quickshell) name="quickshell"; repo="https://github.com/quickshell-mirror/quickshell.git"; bin="quickshell" ;;
        */*) name="${raw##*/}"; bin="$name"; [[ "$raw" == http*://* || "$raw" == git@* ]] && repo="${raw}.git" || repo="https://github.com/${raw}.git" ;;
        *) set_live 0 error "$raw" 100 "[FAIL] Unknown target '$raw' (paste a URL https://... or GitHub owner/repo)"; return 2 ;;
    esac
    [[ "$dry" -eq 1 ]] && { printf '[DRY-RUN] Would forge %s (%s -> %s/%s) with -march=%s (%s threads, linker=%s)\n  Deps: %s\n' "$name" "$repo" "$BIN_DIR" "$bin" "$MARCH_TARGET" "$CPU_JOBS" "$LINKER" "$(preset_deps "$name")"; return 0; }
    if [[ "$force" -eq 0 && -x "$BIN_DIR/$bin" && -f "$STAMP_DIR/$name.stamp" ]] && grep -q "^arch=$MARCH_TARGET$" "$STAMP_DIR/$name.stamp" 2>/dev/null; then
        if [[ "${CHECK_REMOTE:-0}" -eq 1 ]]; then
            local rem_sha cur_sha; cur_sha="$(awk -F= '/^commit=/{print $2}' "$STAMP_DIR/$name.stamp" 2>/dev/null || true)"
            [[ -n "$local_src" ]] && rem_sha="$(git -C "$local_src" rev-parse HEAD 2>/dev/null || true)" || rem_sha="$(timeout 5 git ls-remote "$repo" "${git_branch:-HEAD}" 2>/dev/null | awk '{print $1; exit}' || true)"
            if [[ -z "$rem_sha" || -z "$cur_sha" || "$rem_sha" == "$cur_sha" ]]; then
                [[ -z "$cur_sha" && -n "$rem_sha" ]] && printf 'commit=%s\n' "$rem_sha" >> "$STAMP_DIR/$name.stamp"
                echo "[OK] $name ($BIN_DIR/$bin) is already up-to-date for $MARCH_TARGET" | tee -a "$FORGE_LOG"; return 0
            fi
        else set_live 0 ok "$name" 100 "[OK] $name ($BIN_DIR/$bin) is already forged for $MARCH_TARGET (use --force to rebuild)"; return 0; fi
    fi
    [[ "${IN_UPDATE:-0}" -eq 0 ]] && : > "$FORGE_LOG"; set_live 1 running "$name" 15 "[1/4] Checking & staging build headers for $name..."
    # shellcheck disable=SC2046
    ensure_pkgs $(preset_deps "$name")
    [[ -n "$local_src" ]] && { forge_generic_dir "$local_src" "$name" "$repo" "$bin" "$cargo_args"; return 0; }
    CURRENT_TMP="$(mktemp -d -p "$TMP_BUILD")"; set_live 1 running "$name" 35 "[2/4] Cloning $repo ${git_branch:+($git_branch)}..."
    if [[ -n "$git_branch" ]]; then git clone --depth 1 --branch "$git_branch" --recurse-submodules --shallow-submodules "$repo" "$CURRENT_TMP/src" 2>&1 | tee -a "$FORGE_LOG" || git clone --depth 1 --recurse-submodules --shallow-submodules "$repo" "$CURRENT_TMP/src" 2>&1 | tee -a "$FORGE_LOG"
    else git clone --depth 1 --recurse-submodules --shallow-submodules "$repo" "$CURRENT_TMP/src" 2>&1 | tee -a "$FORGE_LOG"; fi
    forge_generic_dir "$CURRENT_TMP/src" "$name" "$repo" "$bin" "$cargo_args"; rm -rf "$CURRENT_TMP"; CURRENT_TMP=""
}
remove_forge() {
    local tgt="${1#sf-webapp-}"; tgt="${tgt,,}"; local aid="sf-webapp-$tgt"
    command -v superdock >/dev/null 2>&1 && superdock unpin "$aid" >/dev/null 2>&1 || true
    rm -f "$BIN_DIR/$aid" "$HOME/.local/share/applications/$aid.desktop" "$STAMP_DIR/$tgt.stamp" && rm -rf "${WEBAPP_DIR:?}/$tgt" "$HOME/.local/share/icons/hicolor"/*/apps/"$aid.png"
    python3 -c 'import json,os,sys; p,k=sys.argv[1:3]; d=json.load(open(p)) if os.path.exists(p) else {}; d.get("targets",{}).pop(k,None); json.dump(d,open(p,"w"),indent=2)' "$FORGE_JSON" "$tgt" 2>/dev/null || true
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true; set_live 0 ok "$tgt" 100 "[OK] Removed forged target '$tgt'"
}
update_forge() {
    local dry="${1:-0}" src_list="" wa_list="" t u nm pm
    src_list="$( { [[ -d "$STAMP_DIR" ]] && find "$STAMP_DIR" -maxdepth 1 -name '*.stamp' -printf '%f\n' | sed 's/\.stamp$//'; [[ -f "$FORGE_JSON" ]] && python3 -c 'import json,sys; d=json.load(open(sys.argv[1])).get("targets",{}); print("\n".join(v.get("repo",k) if "/" in k else k for k,v in d.items() if v.get("type")!="webapp"))' "$FORGE_JSON" 2>/dev/null; } | awk 'NF && !seen[$0]++' )"
    [[ -f "$FORGE_JSON" ]] && wa_list="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])).get("targets",{}); [print(v.get("url","")+"|"+v.get("name","")+"|"+v.get("profile","shared")) for v in d.values() if v.get("type")=="webapp" and v.get("url")]' "$FORGE_JSON" 2>/dev/null || true)"
    [[ "$dry" -eq 1 ]] && { printf '[DRY-RUN] superforge update:\n  1. System RPMs : dnf upgrade --refresh -y\n  2. Runtimes    : flatpak update -y && rustup update\n  3. Source      : %s\n  4. WebApps     : %s\n' "${src_list//$'\n'/ }" "${wa_list//$'\n'/ }"; return 0; }
    : > "$FORGE_LOG"; IN_UPDATE=1; set_live 1 running "system (dnf)" 10 "[1/4] Checking Fedora system packages (dnf upgrade)..."
    if [[ $EUID -eq 0 ]]; then dnf upgrade --refresh -y 2>&1 | tee -a "$FORGE_LOG" || true
    elif sudo -n true 2>/dev/null; then sudo -n dnf upgrade --refresh -y 2>&1 | tee -a "$FORGE_LOG" || true
    elif [[ -t 0 ]]; then sudo dnf upgrade --refresh -y 2>&1 | tee -a "$FORGE_LOG" || true; fi
    set_live 1 running "toolchains & flatpak" 30 "[2/4] Updating Flatpak runtimes & Rust compiler toolchain..."
    command -v flatpak >/dev/null 2>&1 && flatpak update -y --noninteractive 2>&1 | tee -a "$FORGE_LOG" || true; command -v rustup >/dev/null 2>&1 && rustup update 2>&1 | tee -a "$FORGE_LOG" || true
    set_live 1 running "source builds" 55 "[3/4] Checking forged native binaries (-O3 -march=native)..."
    for t in $src_list; do CHECK_REMOTE=1 forge_target "$t" "$FORCE" 0; done
    set_live 1 running "webapps" 85 "[4/4] Refreshing chromeless Firefox WebApp icons & profiles..."
    while IFS='|' read -r u nm pm; do [[ -n "$u" ]] && NO_PIN=1 forge_webapp "$u" "$nm" "${pm:-shared}" 0; done <<< "$wa_list"
    IN_UPDATE=0; set_live 0 ok "update complete" 100 "[OK] Updated system packages, toolchains, forged binaries & WebApps"
    command -v notify-send >/dev/null 2>&1 && notify-send -a "SF FORGE" "Forge Update Complete" "Verified toolchains, forged binaries, and Firefox WebApps." || true
}
FORCE=0; DRY=0; INSTALL_DEPS=0; PROF_MODE="shared"; NO_PIN=0; ARGS=()
for a in "$@"; do
    case "$a" in --force|-f) FORCE=1 ;; --dry-run|-n) DRY=1 ;; --install) INSTALL_DEPS=1 ;; --isolated) PROF_MODE="isolated" ;; --shared) PROF_MODE="shared" ;; --no-pin) NO_PIN=1 ;; --name) ;; -h|--help|help) sed -n '7,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;; *) ARGS+=("$a") ;; esac
done
CMD="${ARGS[0]:-status}"
# shellcheck disable=SC2046
case "$CMD" in
    deps) detect_hw; t="${ARGS[1]:-hyprland}"; echo "Dependencies for $t: $(preset_deps "$t")"; [[ $INSTALL_DEPS -eq 1 ]] && ensure_pkgs $(preset_deps "$t") || true ;;
    webapp|web|pwa) forge_webapp "${ARGS[1]:?url required}" "${ARGS[2]:-}" "$PROF_MODE" "$DRY" ;;
    remove|rm|uninstall|unforge) remove_forge "${ARGS[1]:?target required}" ;;
    update|upgrade) update_forge "$DRY" ;;
    rebuild) [[ -f "$FORGE_JSON" ]] && for t in $(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])).get("targets",{}); print(" ".join(k for k,v in d.items() if v.get("type")!="webapp"))' "$FORGE_JSON"); do forge_target "$t" 1 "$DRY"; done ;;
    *) if [[ ${#ARGS[@]} -ge 2 && ( "${ARGS[0]}" =~ ^https?:// || ( "${ARGS[0]}" == *.* && "${ARGS[0]}" != */* ) ) ]]; then forge_webapp "${ARGS[0]}" "${ARGS[1]}" "$PROF_MODE" "$DRY"; else for t in "${ARGS[@]}"; do forge_target "$t" "$FORCE" "$DRY" "" "$PROF_MODE"; done; fi ;;
esac
