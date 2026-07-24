#!/usr/bin/env bash
# Buitenzorg OS — one-command quick start (Linux / macOS).
#
# Installs every dependency needed to build and boot Buitenzorg OS, then builds
# the disk image and launches it in QEMU. Safe to re-run: each step is skipped
# if the tool is already present.
#
#   Dependencies handled: Rust (rustup + nightly toolchain + bare-metal target),
#   .NET SDK, QEMU, and bflat (the C#->native compiler, downloaded into tools/).
#
# Usage:
#   ./scripts/quickstart.sh              # install deps, build, boot in QEMU
#   ./scripts/quickstart.sh --no-run     # install + build only
#   ./scripts/quickstart.sh --smoke      # install + build + headless self-test
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

info() { printf '\033[36m==> %s\033[0m\n' "$1"; }
ok()   { printf '\033[32m  ok: %s\033[0m\n' "$1"; }
warn() { printf '\033[33m  ! %s\033[0m\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }

NO_RUN=0; SMOKE=0
for a in "$@"; do
  case "$a" in
    --no-run) NO_RUN=1 ;;
    --smoke) SMOKE=1 ;;
    *) echo "unknown arg: $a"; exit 1 ;;
  esac
done

# Pick the system package manager for QEMU / prerequisites.
pkg_install() { # pkg_install <pkg>
  if have apt-get; then sudo apt-get update -y && sudo apt-get install -y "$1";
  elif have dnf; then sudo dnf install -y "$1";
  elif have pacman; then sudo pacman -Sy --noconfirm "$1";
  elif have zypper; then sudo zypper install -y "$1";
  elif have brew; then brew install "$1";
  else warn "no known package manager; install '$1' manually."; return 1; fi
}

# --- 1. Rust (rustup) --------------------------------------------------------
info "Checking Rust toolchain (rustup)"
if ! have rustup; then
  info "Installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
have rustup || { warn "open a new shell (source ~/.cargo/env) and re-run."; exit 1; }
ok "rustup present"
# The kernel pins nightly + x86_64-unknown-none + rust-src via
# kernel/rust-toolchain.toml; rustup installs it on the first build.
rustup show >/dev/null 2>&1 || true
ok "Rust ready"

# --- 2. .NET SDK -------------------------------------------------------------
info "Checking .NET SDK"
if ! have dotnet; then
  info "Installing .NET SDK via the official install script (~/.dotnet)..."
  curl -sSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh
  bash /tmp/dotnet-install.sh --channel STS --install-dir "$HOME/.dotnet"
  export PATH="$HOME/.dotnet:$PATH"
fi
if have dotnet; then ok ".NET SDK: $(dotnet --version)"; else warn ".NET SDK not detected; C# ABI tests are skipped (kernel still builds)."; fi

# --- 3. QEMU -----------------------------------------------------------------
info "Checking QEMU"
if ! have qemu-system-x86_64; then
  info "Installing QEMU..."
  pkg_install qemu-system-x86 || pkg_install qemu || true
fi
if have qemu-system-x86_64; then ok "QEMU present"; else warn "QEMU not detected; install qemu-system-x86 and re-run (or set QEMU=...)."; fi

# --- 4. bflat (C# -> native) -------------------------------------------------
info "Checking bflat (tools/bflat)"
bflat="$root/tools/bflat/bflat"
if [ ! -x "$bflat" ]; then
  info "Downloading the latest bflat release for linux-glibc-x64..."
  url="$(curl -sSL https://api.github.com/repos/bflattened/bflat/releases/latest \
        | grep -oE 'https://[^"]*linux-glibc-x64\.tar\.gz' | head -1)"
  [ -n "$url" ] || { warn "could not find a linux-glibc-x64 bflat asset; download it manually into tools/bflat/."; }
  if [ -n "$url" ]; then
    mkdir -p "$root/tools/bflat"
    tmp="/tmp/bflat_dl"; rm -rf "$tmp"; mkdir -p "$tmp"
    ( cd "$tmp" && curl -sSL "$url" -o pkg && (tar xzf pkg 2>/dev/null || unzip -q pkg) )
    # Move the extracted contents (bflat + libs) into tools/bflat.
    cp -r "$tmp"/*/. "$root/tools/bflat/" 2>/dev/null || cp -r "$tmp"/. "$root/tools/bflat/"
    chmod +x "$bflat" 2>/dev/null || true
  fi
fi
[ -x "$bflat" ] && ok "bflat present" || warn "bflat missing; the C# apps won't build (kernel still boots without them)."

# --- 5. Build + run ----------------------------------------------------------
info "Building the C# userland apps"
bash "$root/scripts/build-hello-csharp.sh" || warn "C# build skipped/failed (need bflat)."
info "Building the disk image (kernel + bootloader)"
( cd "$root/kernel" && cargo run --release -p bzimage -- --out ../dist )
ok "Images built: dist/buitenzorg-bios.img, dist/buitenzorg-uefi.img"

if [ "$SMOKE" = 1 ]; then
  info "Running the headless smoke test (all 4 boot media)"
  bash "$root/scripts/smoke-test.sh"
elif [ "$NO_RUN" = 0 ]; then
  info "Booting Buitenzorg OS in QEMU (close the window or Ctrl+C to stop)"
  qemu-system-x86_64 -drive "format=raw,file=$root/dist/buitenzorg-bios.img" \
    -m 512M -audiodev none,id=snd0 -device AC97,audiodev=snd0 -serial stdio
else
  ok "Done (build only). Boot it with QEMU using dist/buitenzorg-bios.img."
fi
