#!/usr/bin/env bash
# Cross-build one release platform and package it with its receipt. Linux aarch64 and both
# macOS platforms are linked by cargo-zigbuild (the macOS ones against the macOS SDK that the
# runner provides, CI_MACOS_SDK: macos-sdk-env.sh); the macOS clone only tests them. Both Windows platforms are built for the one gnullvm ABI
# with clang and lld and the mingw-w64 UCRT runtime of llvm-mingw (plain cargo build,
# no Microsoft SDK, free licences). The result is built, never run here: ARM64 has no
# native executor and is never emulated, and the Windows x86_64 executable is run
# later, as this very file, in its Windows clone (.agent/scripts/windows-test.sh).
set -Eeuo pipefail
# Whatever fails here says so, with its line and command, on stdout as well: the job log of the
# release run drops the standard error of a failing step, and a silent exit 1 cost a run (105).
trap 'printf "cross-release.sh (%s): line %s: %s exited %s\n" "${platform:-?}" "$LINENO" "$BASH_COMMAND" "$?"' ERR
repo_root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
platform="${1:?Usage: cross-release.sh PLATFORM OUTPUT}"
output="${2:?Missing package output directory}"
[[ -z ${GH_TOKEN:-} ]] || { echo 'Publication credentials must not reach the cross build.' >&2; exit 2; }
if [[ ${FORGEJO_ACTIONS:-} == true ]]; then
  [[ -n ${CARGO_TARGET_DIR:-} && ${CCID_TARGET_LOCK_HELD:-} == "$CARGO_TARGET_DIR" ]] || {
    echo 'Forgejo cross builds require the verified ccid target lock.' >&2; exit 2;
  }
fi
exe=cv
driver=zigbuild
case "$platform" in
  linux-aarch64) triple=aarch64-unknown-linux-gnu; zig_target="$triple.2.39" ;;
  windows-x86_64) triple=x86_64-pc-windows-gnullvm; driver=llvm-mingw; exe=cv.exe ;;
  windows-arm64) triple=aarch64-pc-windows-gnullvm; driver=llvm-mingw; exe=cv.exe ;;
  macos-arm64) triple=aarch64-apple-darwin; driver=zigbuild-apple; zig_target="$triple" ;;
  macos-x86_64) triple=x86_64-apple-darwin; driver=zigbuild-apple; zig_target="$triple" ;;
  linux-x86_64) echo 'Linux x86_64 is built and run natively: native-release.sh.' >&2; exit 2 ;;
  *) echo "Unknown cross platform: $platform" >&2; exit 2 ;;
esac
if [[ $driver == zigbuild || $driver == zigbuild-apple ]]; then
  for tool in cargo-zigbuild zig; do
    command -v "$tool" >/dev/null || { echo "Missing cross toolchain: $tool" >&2; exit 2; }
  done
  if [[ $driver == zigbuild-apple ]]; then
    # shellcheck source=.agent/scripts/macos-sdk-env.sh
    source .agent/scripts/macos-sdk-env.sh
    ccvl_macos_sdk_env
  fi
else
  # shellcheck source=.agent/scripts/llvm-mingw-env.sh
  source .agent/scripts/llvm-mingw-env.sh
  # clang links the gnullvm targets and compiles their C and assembly (the psm crate, for
  # one); the Windows test suite names the same tools (windows-test.sh).
  ccvl_llvm_mingw_env "$triple"
fi
# shellcheck source=.agent/scripts/rust-toolchain.sh
source .agent/scripts/rust-toolchain.sh
ccvl_select_rust_toolchain
# The target's standard library belongs to the selected toolchain. The release
# job provisions it; a toolchain that still lacks it gets it from its own rustup.
sysroot="$("${CCVL_RUSTC_COMMAND[@]}" --print sysroot)"
if [[ ! -d $sysroot/lib/rustlib/$triple/lib ]]; then
  if [[ ${CCVL_RUSTC_COMMAND[0]##*/} == rustup && ${CCVL_RUSTC_COMMAND[1]} == run ]]; then
    "${CCVL_RUSTC_COMMAND[0]}" target add "$triple" --toolchain "${CCVL_RUSTC_COMMAND[2]}"
  else
    echo "The Rust toolchain has no $triple standard library and cannot add one." >&2; exit 2
  fi
fi
# Run the build with its output kept in a log; when it fails, print the end of
# that log on both streams, so a failed stage never exits without its error.
build_logged() {
  local log status=0
  log="$(mktemp "${TMPDIR:-/tmp}/cross-build.XXXXXX")"
  # Run in build_cwd when one is set, else here.
  (cd "${build_cwd:-$PWD}" && "$@") >"$log" 2>&1 || status=$?
  cat "$log"
  if ((status != 0)); then
    {
      printf 'Cross build of %s failed (exit %s); the end of its log:\n' "$platform" "$status"
      tail -n 60 "$log"
    } | tee /dev/stderr
    rm -f "$log"
    exit "$status"
  fi
  rm -f "$log"
}
if [[ $driver == zigbuild || $driver == zigbuild-apple ]]; then
  # The linker probes of the zig toolchain write files (a.o) into their working directory when
  # no output is named (release run 109 found `a.o` in the checkout). The build runs in a fresh
  # scratch directory instead, with the manifest named by path, so a stray file cannot dirty
  # the source tree the evidence verifies.
  build_cwd="$(mktemp -d "${TMPDIR:-/tmp}/cross-cwd.XXXXXX")"
  # rustc and the linker run in the workspace root whatever the directory above; remove what the
  # zig linker leaves there (a.o) and nothing else (stray-outputs.sh).
  # shellcheck source=.agent/scripts/stray-outputs.sh
  source .agent/scripts/stray-outputs.sh
  tree_before="$(mktemp "${TMPDIR:-/tmp}/tree-before.XXXXXX")"
  ccvl_tree_snapshot "$tree_before"
  if [[ $driver == zigbuild-apple ]]; then
    printf 'zig %s, cargo-zigbuild %s\n' "$(zig version)" "$(cargo-zigbuild --version | awk '{print $NF}')"
  fi
  build_logged "${CCVL_CARGO_COMMAND[@]}" zigbuild --locked --release \
    --manifest-path "$repo_root/Cargo.toml" --config "$repo_root/.cargo/release-profile.toml" --target "$zig_target"
  if [[ -n "$(ls -A "$build_cwd")" ]]; then
    printf 'cross-release.sh (%s): the build left files in its working directory (kept out of the checkout): %s\n' "$platform" "$(find "$build_cwd" -mindepth 1 -maxdepth 1 -exec basename {} + | tr "\n" " ")"
  fi
  find "$build_cwd" -delete
  unset build_cwd
  ccvl_remove_stray_objects "$tree_before" || true
  rm -f "$tree_before"
  tool="cargo-zigbuild $(cargo-zigbuild --version | awk '{print $NF}'), zig $(zig version)"
  if [[ $driver == zigbuild-apple ]]; then tool="$tool, $(ccvl_macos_sdk_identity)"; fi
else
  build_logged "${CCVL_CARGO_COMMAND[@]}" build --locked --release \
    --config .cargo/release-profile.toml --target "$triple"
  tool="$(ccvl_llvm_mingw_identity "$triple")"
fi
binary="${CARGO_TARGET_DIR:-$repo_root/target}/$triple/release/$exe"
if [[ $driver == zigbuild-apple ]]; then
  # Never run here; at least say what it is: the Mach-O header names the architecture.
  arch="$(ccvl_macho_arch "$binary")"
  case "$platform:$arch" in
    macos-x86_64:x86_64|macos-arm64:arm64) ;;
    *) echo "$binary is not the Mach-O executable of $platform (header says: $arch)." | tee /dev/stderr; exit 2 ;;
  esac
fi
if [[ $driver == llvm-mingw ]]; then
  # A Windows executable of this ABI imports Windows system libraries only. A mingw
  # runtime DLL (libunwind, libc++, libwinpthread) is not on a clean Windows, and an ARM64
  # build is never run here, so this is the one place that can tell.
  imports="$(llvm-objdump -p "$binary" | sed -n 's/^[[:space:]]*DLL Name: //p')"
  [[ -n $imports ]] || { echo "No import table read from $binary." >&2; exit 2; }
  if grep -i -E '^(lib|msys|mingw)' <<<"$imports" >&2; then
    echo "$binary imports a mingw runtime DLL that a clean Windows does not have." >&2; exit 2
  fi
fi
# Nothing a build does may write into the source tree: the evidence below refuses a dirty
# checkout. Say which paths, here, before the next stage has to guess.
if [[ -d .git ]] && dirt="$(git status --porcelain --untracked-files=normal)" && [[ -n $dirt ]]; then
  printf 'cross-release.sh (%s): the build left the source tree dirty:\n%s\n' "$platform" "$dirt"
fi
# The tested native executable of this run serves the evidence command when it
# exists; otherwise the command runs from the checkout.
if [[ -x "$output/cv-linux-x86_64" ]]; then export CCVL_RELEASE_EVIDENCE_BIN="$output/cv-linux-x86_64"; fi
# shellcheck source=.agent/scripts/release-evidence.sh
source .agent/scripts/release-evidence.sh
ccvl_prepare_release_evidence
evidence_log="$(mktemp "${TMPDIR:-/tmp}/cross-evidence.XXXXXX")"
evidence_status=0
"${CCVL_RELEASE_EVIDENCE_COMMAND[@]}" cross \
  --platform "$platform" --binary "$binary" --host "$triple" \
  --rustc "$("${CCVL_RUSTC_COMMAND[@]}" --version)" \
  --cargo "$("${CCVL_CARGO_COMMAND[@]}" --version)" \
  --tool "$tool" --output "$output" 2>"$evidence_log" || evidence_status=$?
cat "$evidence_log" >&2
if ((evidence_status != 0)); then
  printf 'release-evidence cross for %s failed (exit %s): %s\n' "$platform" "$evidence_status" "$(tr '\n' ' ' <"$evidence_log")"
  rm -f "$evidence_log"
  exit "$evidence_status"
fi
rm -f "$evidence_log"
