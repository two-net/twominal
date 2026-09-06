#!/bin/sh

set -eu

fail() {
  printf 'Twominal: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [ "$twominal_mounted" = true ]; then
    if ! hdiutil detach "$twominal_tmp/mount"; then
      printf 'Could not eject the installer; temporary files remain at %s\n' "$twominal_tmp" >&2
      return
    fi
  fi
  rm -rf -- "$twominal_tmp"
}

main() {
  [ "$(uname -s)" = Darwin ] || fail 'This installer requires macOS.'
  [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] ||
    fail 'This release is available for Apple Silicon Macs only.'

  twominal_version=0.1.1
  twominal_asset="Twominal_${twominal_version}_aarch64.dmg"
  twominal_release="https://github.com/two-net/twominal/releases/download/v${twominal_version}"
  twominal_install_dir="${TWOMINAL_INSTALL_DIR:-$HOME/Applications}"
  case "$twominal_install_dir" in
    /*) ;;
    *) fail 'TWOMINAL_INSTALL_DIR must be an absolute path.' ;;
  esac
  twominal_app="$twominal_install_dir/Twominal.app"
  if [ -e "$twominal_app" ] || [ -L "$twominal_app" ]; then
    fail "An app already exists at $twominal_app; move it aside before installing."
  fi

  mkdir -p "$twominal_install_dir"
  # Stage on the destination volume so the final move does not copy a partial app.
  twominal_tmp="$(mktemp -d "$twominal_install_dir/.twominal.XXXXXX")"
  twominal_mounted=false
  trap cleanup EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM

  printf 'Downloading Twominal %s...\n' "$twominal_version"
  curl --fail --location --show-error --silent --proto '=https' --proto-redir '=https' \
    --output "$twominal_tmp/$twominal_asset" "$twominal_release/$twominal_asset"
  curl --fail --location --show-error --silent --proto '=https' --proto-redir '=https' \
    --output "$twominal_tmp/SHA256SUMS.txt" "$twominal_release/SHA256SUMS.txt"

  twominal_checksum="$(awk -v asset="$twominal_asset" '
    $2 == asset { checksum = $1; count++ }
    END { if (count != 1) exit 1; print checksum }
  ' "$twominal_tmp/SHA256SUMS.txt")" || fail 'The release checksum entry is missing or ambiguous.'
  (
    cd "$twominal_tmp"
    printf '%s  %s\n' "$twominal_checksum" "$twominal_asset" | shasum -a 256 --check -
  ) || fail 'The downloaded installer failed checksum verification.'

  hdiutil attach -readonly -nobrowse -noautoopen \
    -mountpoint "$twominal_tmp/mount" "$twominal_tmp/$twominal_asset"
  twominal_mounted=true
  ditto "$twominal_tmp/mount/Twominal.app" "$twominal_tmp/Twominal.app"
  codesign --verify --deep --strict "$twominal_tmp/Twominal.app"
  hdiutil detach "$twominal_tmp/mount"
  twominal_mounted=false

  mv -n "$twominal_tmp/Twominal.app" "$twominal_install_dir/"
  [ ! -e "$twominal_tmp/Twominal.app" ] || fail 'The destination app already exists.'
  printf 'Installed Twominal %s at %s. Open it to start.\n' "$twominal_version" "$twominal_app"
}

main "$@"
