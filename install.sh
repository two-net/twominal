#!/bin/sh
# Install Twominal on macOS from GitHub Releases.
#
#   curl --proto '=https' --proto-redir '=https' -fsSL \
#     https://raw.githubusercontent.com/two-net/twominal/main/install.sh | sh
#
# Optional:
#   TWOMINAL_VERSION=0.1.0     pin a release (default: latest)
#   TWOMINAL_INSTALL_DIR=...   absolute destination (default: /Applications)
#   TWOMINAL_FORCE=1           replace an existing Twominal.app
#
# curl does not apply the browser quarantine flag, so this path often skips
# Gatekeeper's Open Anyway prompt. The app is still ad-hoc signed and not
# notarized; MDM policies can still block it.

set -eu

fail() {
  printf 'Twominal: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [ "${twominal_mounted:-false}" = true ]; then
    if ! hdiutil detach "$twominal_tmp/mount"; then
      printf 'Could not eject the installer; temporary files remain at %s\n' "$twominal_tmp" >&2
      return
    fi
  fi
  if [ -n "${twominal_stage:-}" ]; then
    dest rm -rf -- "$twominal_stage"
  fi
  if [ -n "${twominal_tmp:-}" ]; then
    rm -rf -- "$twominal_tmp"
  fi
}

dest() {
  if [ "${twominal_use_sudo:-false}" = true ]; then
    sudo "$@"
  else
    "$@"
  fi
}

need_sudo() {
  if [ ! -d "$twominal_install_dir" ]; then
    parent="$(dirname "$twominal_install_dir")"
    [ -w "$parent" ] && return 1
    return 0
  fi
  [ -w "$twominal_install_dir" ] || return 0
  if [ -e "$twominal_app" ] && [ ! -w "$twominal_app" ]; then
    return 0
  fi
  return 1
}

asset_suffix() {
  if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
    printf 'aarch64.dmg'
    return
  fi
  case "$(uname -m)" in
    x86_64) printf 'x64.dmg' ;;
    *) fail "Unsupported macOS architecture: $(uname -m)" ;;
  esac
}

main() {
  [ "$(uname -s)" = Darwin ] || fail 'This installer requires macOS.'

  twominal_suffix="$(asset_suffix)"
  twominal_repo='two-net/twominal'
  twominal_install_dir="${TWOMINAL_INSTALL_DIR:-/Applications}"
  case "$twominal_install_dir" in
    /*) ;;
    *) fail 'TWOMINAL_INSTALL_DIR must be an absolute path.' ;;
  esac
  twominal_app="$twominal_install_dir/Twominal.app"
  if [ -e "$twominal_app" ] || [ -L "$twominal_app" ]; then
    [ "${TWOMINAL_FORCE:-}" = 1 ] ||
      fail "An app already exists at $twominal_app; move it aside or set TWOMINAL_FORCE=1."
  fi

  twominal_use_sudo=false
  if [ "$(id -u)" -ne 0 ] && need_sudo; then
    twominal_use_sudo=true
    printf 'Installing to %s requires administrator access.\n' "$twominal_install_dir"
  fi

  if [ -n "${TWOMINAL_VERSION:-}" ]; then
    twominal_tag="v${TWOMINAL_VERSION#v}"
    twominal_release="https://github.com/${twominal_repo}/releases/download/${twominal_tag}"
  else
    twominal_release="https://github.com/${twominal_repo}/releases/latest/download"
  fi

  twominal_tmp="$(mktemp -d "${TMPDIR:-/tmp}/twominal.XXXXXX")"
  twominal_stage=
  twominal_mounted=false
  trap cleanup EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM

  printf 'Downloading Twominal (%s)...\n' "$twominal_suffix"
  curl --fail --location --show-error --silent --proto '=https' --proto-redir '=https' \
    --output "$twominal_tmp/SHA256SUMS.txt" "$twominal_release/SHA256SUMS.txt"

  twominal_parsed="$(awk -v suffix="$twominal_suffix" '
    length($2) >= length(suffix) && substr($2, length($2) - length(suffix) + 1) == suffix {
      asset = $2
      checksum = $1
      count++
    }
    END {
      if (count != 1) exit 1
      print asset
      print checksum
    }
  ' "$twominal_tmp/SHA256SUMS.txt")" || fail 'The release checksum entry is missing or ambiguous.'
  twominal_asset="$(printf '%s\n' "$twominal_parsed" | awk 'NR==1')"
  twominal_checksum="$(printf '%s\n' "$twominal_parsed" | awk 'NR==2')"
  [ -n "$twominal_asset" ] && [ -n "$twominal_checksum" ] ||
    fail 'The release checksum entry is missing or ambiguous.'

  curl --fail --location --show-error --silent --proto '=https' --proto-redir '=https' \
    --output "$twominal_tmp/$twominal_asset" "$twominal_release/$twominal_asset"

  (
    cd "$twominal_tmp"
    printf '%s  %s\n' "$twominal_checksum" "$twominal_asset" | shasum -a 256 --check -
  ) || fail 'The downloaded installer failed checksum verification.'

  hdiutil attach -readonly -nobrowse -noautoopen \
    -mountpoint "$twominal_tmp/mount" "$twominal_tmp/$twominal_asset"
  twominal_mounted=true
  [ -d "$twominal_tmp/mount/Twominal.app" ] || fail 'The disk image does not contain Twominal.app.'
  ditto "$twominal_tmp/mount/Twominal.app" "$twominal_tmp/Twominal.app"
  codesign --verify --deep --strict "$twominal_tmp/Twominal.app" ||
    fail 'The app failed code signature verification.'
  hdiutil detach "$twominal_tmp/mount"
  twominal_mounted=false

  dest mkdir -p "$twominal_install_dir"
  if [ -e "$twominal_app" ] || [ -L "$twominal_app" ]; then
    dest rm -rf -- "$twominal_app"
  fi
  twominal_stage="$(dest mktemp -d "$twominal_install_dir/.twominal.XXXXXX")"
  dest ditto "$twominal_tmp/Twominal.app" "$twominal_stage/Twominal.app"
  dest mv -n "$twominal_stage/Twominal.app" "$twominal_install_dir/"
  [ ! -e "$twominal_stage/Twominal.app" ] || fail 'The destination app already exists.'
  dest rmdir "$twominal_stage"
  twominal_stage=
  printf 'Installed %s at %s. Open it to start.\n' "$twominal_asset" "$twominal_app"
}

main "$@"
