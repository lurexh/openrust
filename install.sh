#!/bin/sh
# one-line installer: curl -fsSL https://raw.githubusercontent.com/lurexh/openrust/main/install.sh | sh
set -e

repo="lurexh/openrust"
os="$(uname -s)"
arch="$(uname -m)"
if [ "$os" != "Linux" ]; then
  echo "unsupported OS: $os (use install.ps1 on Windows)" >&2
  exit 1
fi
case "$arch" in
  aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
  x86_64)        target="x86_64-unknown-linux-musl" ;;
  *) echo "unsupported architecture: $arch" >&2; exit 1 ;;
esac

fetch() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1"; else wget -qO- "$1"; fi
}
fetch_file() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL -o "$2" "$1"; else wget -qO "$2" "$1"; fi
}

echo "==> resolving latest release for $target"
asset_url="$(fetch "https://api.github.com/repos/$repo/releases/latest" \
  | grep '"browser_download_url"' | grep "$target.tar.gz" \
  | head -n1 | cut -d'"' -f4)"
if [ -z "$asset_url" ]; then
  echo "error: could not find a release asset for $target" >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "==> downloading $asset_url"
fetch_file "$asset_url" "$tmp/oc-rs.tar.gz"
fetch_file "$asset_url.sha256" "$tmp/oc-rs.tar.gz.sha256"
expected="$(cut -d' ' -f1 "$tmp/oc-rs.tar.gz.sha256")"
actual="$(sha256sum "$tmp/oc-rs.tar.gz" | cut -d' ' -f1)"
[ "$expected" = "$actual" ] || { echo "error: checksum mismatch" >&2; exit 1; }
tar xzf "$tmp/oc-rs.tar.gz" -C "$tmp"

bin_dir="${PREFIX:-$HOME/.local}/bin"
mkdir -p "$bin_dir"
install -m 755 "$tmp/oc-rs" "$bin_dir/oc-rs"

echo "==> installed to $bin_dir/oc-rs"
command -v oc-rs >/dev/null 2>&1 && {
  echo "    run it:  oc-rs"
} || echo "    add $bin_dir to PATH, then run: oc-rs"
