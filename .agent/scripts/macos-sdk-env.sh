#!/usr/bin/env bash
# Sourced, not run: the macOS SDK the Linux release job links the darwin targets against.
#
# cargo-zigbuild brings clang, lld and the libc headers of Zig, but not Apple's system
# frameworks (CoreFoundation, Security, ...): without an SDK the link fails with "unable to
# find system framework". The SDK is provisioned on the runner (the Command Line Tools of the
# macOS clone, copied) and named by CI_MACOS_SDK, the directory of the SDK itself
# (MacOSX*.sdk). The cross build of macos-x86_64 and macos-arm64 and the test bundle of
# macos-x86_64 (macos-test-bundle.sh) both use it, so the executable that ships and the tests
# that vouch for it are linked the same way.

# Where the runners keep the SDK (a symlink to the current MacOSX*.sdk). A slot picks
# CI_MACOS_SDK up at its next idle restart; until then this path is used when it exists.
# (Overridable so that a test can point it at nothing.)
: "${CCVL_MACOS_SDK_DEFAULT:=/solid/apps/cache/macos-sdk/current}"

# The one absolute path the macOS test executables are built from and run at. Cargo bakes the
# manifest path as given (a symlink does not help), so the Linux build host copies the source
# tree here and the Mac recreates exactly this path. /var/tmp is user-writable on both
# (/private/var/tmp on macOS, sticky) and is a disk, not tmpfs, on srv. The root directory
# is overridable for tests only.
: "${CCVL_MACOS_TEST_ROOT:=/var/tmp/ccvl-xtest}"
# shellcheck disable=SC2034 # read by the scripts that source this file
CCVL_MACOS_TEST_SRC="$CCVL_MACOS_TEST_ROOT/src"
# A tree this scheme made has this file beside it (not inside: the tree is the source, and an
# extra file in it would change what the tests see); any other directory is never written over.
# On the Linux build host the file holds the token of the run that made the tree.
# shellcheck disable=SC2034 # read by the scripts that source this file
CCVL_MACOS_TEST_SRC_MARKER="$CCVL_MACOS_TEST_ROOT/src.made-by-ccvl"

# ccvl_macos_sdk_env: validate CI_MACOS_SDK (or the default location) and export SDKROOT, or
# stop with the reason.
ccvl_macos_sdk_env() {
  local sdk="${CI_MACOS_SDK:-}"
  if [[ -z $sdk && -d $CCVL_MACOS_SDK_DEFAULT ]]; then sdk="$CCVL_MACOS_SDK_DEFAULT"; fi
  if [[ -z $sdk ]]; then
    echo "CI_MACOS_SDK is not set and $CCVL_MACOS_SDK_DEFAULT does not exist: the macOS targets need the macOS SDK that the runner provides (the directory MacOSX*.sdk)." >&2
    return 2
  fi
  if [[ ! -d $sdk/System/Library/Frameworks/CoreFoundation.framework ]]; then
    echo "CI_MACOS_SDK=$sdk is not a macOS SDK: it has no System/Library/Frameworks/CoreFoundation.framework." >&2
    return 2
  fi
  export SDKROOT="$sdk"
}

# ccvl_macos_sdk_identity: a one-line name of the SDK for the receipt.
ccvl_macos_sdk_identity() {
  local settings="$SDKROOT/SDKSettings.json" version=
  if [[ -r $settings ]] && command -v jq >/dev/null 2>&1; then
    version="$(jq -r '.Version // empty' "$settings" 2>/dev/null || true)"
  fi
  printf 'macOS SDK %s (%s)\n' "${version:-unknown}" "$(basename -- "$SDKROOT")"
}

# ccvl_macho_arch FILE: x86_64, arm64 or "unknown", from the Mach-O header itself (the 64-bit
# little-endian magic cf fa ed fe, then the CPU type), so no `file` binary is needed on the runner.
ccvl_macho_arch() {
  local header
  header="$(od -An -tx1 -N8 -- "$1" 2>/dev/null | tr -d ' \n')"
  case "$header" in
    cffaedfe07000001) echo x86_64 ;;
    cffaedfe0c000001) echo arm64 ;;
    *) echo unknown ;;
  esac
}
