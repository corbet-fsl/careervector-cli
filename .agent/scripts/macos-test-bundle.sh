#!/usr/bin/env bash
# The test executables of macos-x86_64, cross-built on Linux, in one bundle for the macOS clone.
#
#   macos-test-bundle.sh OUTPUT      OUTPUT is the release directory (CCVL_RELEASE_DIR)
#
# Leaves OUTPUT/macos-tests-x86_64.tar.gz in the draft, like the Windows test suite travels to
# its clone (windows-test.sh), with the same rule: a test executable embeds the absolute path
# of the checkout it was built from (CARGO_MANIFEST_DIR, fixtures), so the bundle records that
# path in suite.json and the clone puts the source tree there before it runs them.
#
# The bundle holds, flat:
#   suite.json     {"src": "<fixed absolute path>", "source_commit": ..., "runtime_id": ...,
#                   "files": {"<path in the bundle>": "<sha256>", ...}, "tests": ["<executable>", ...]}
#   tests/<name>   the test executables (profile.test), Mach-O x86_64
#   cv             the debug cv the integration tests drive (CCVL_TEST_BIN in the clone)
#
# Built from a copy of the tree at the fixed path /var/tmp/ccvl-xtest/src (macos-sdk-env.sh: why),
# with cargo-zigbuild against the macOS SDK of the runner; nothing is
# run here and nothing needs the clone's toolchain: it has no cargo, no rustup and no git history.
set +x
set -euo pipefail
repo_root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
output="${1:?Usage: macos-test-bundle.sh OUTPUT}"
triple=x86_64-apple-darwin
[[ -z ${GH_TOKEN:-} ]] || { echo 'Publication credentials must not reach the macOS test build.' >&2; exit 2; }
for tool in cargo-zigbuild zig jq tar od; do
  command -v "$tool" >/dev/null || { echo "Missing cross toolchain: $tool" >&2; exit 2; }
done
# shellcheck source=.agent/scripts/macos-sdk-env.sh
source .agent/scripts/macos-sdk-env.sh
ccvl_macos_sdk_env
# shellcheck source=.agent/scripts/rust-toolchain.sh
source .agent/scripts/rust-toolchain.sh
ccvl_select_rust_toolchain
scratch="$(mktemp -d "${TMPDIR:-/tmp}/ccvl-macos-tests.XXXXXXXX")"
trap 'find "$scratch" -delete' EXIT
src="$CCVL_MACOS_TEST_SRC"
# The tree the executables are built from, at the one fixed path the Mac recreates: the
# tracked files of this checkout, with their LFS objects (the working tree holds them), minus
# .git. macos-test-src.sh says how the claim is made safely; a refusal here deletes nothing,
# because the cleanup is armed only after the claim.
# shellcheck source=.agent/scripts/macos-test-src.sh
source .agent/scripts/macos-test-src.sh
ccvl_xtest_claim || exit $?
trap 'find "$scratch" -delete; ccvl_xtest_release' EXIT
git ls-files -z | tar --null -T - -cf - | tar -xf - -C "$src"

build_json="$scratch/build.json"
started=$SECONDS
status=0
# Run in a scratch directory, the manifest named by path: a stray file of the zig toolchain
# (release run 109: a.o) must land neither in the checkout nor in the copy the tests will see.
build_cwd="$scratch/cwd"
mkdir -p "$build_cwd"
(cd "$build_cwd" && "${CCVL_CARGO_COMMAND[@]}" zigbuild --locked --workspace --all-features --tests --bins \
  --manifest-path "$src/Cargo.toml" --target "$triple" --message-format=json-render-diagnostics) >"$build_json" 2>"$scratch/build.err" || status=$?
if [[ -n "$(ls -A "$build_cwd")" ]]; then
  printf 'macos-test-bundle.sh: the build left files in its working directory (kept out of the tree): %s\n' "$(find "$build_cwd" -mindepth 1 -maxdepth 1 -exec basename {} + | tr "\n" " ")"
fi
if ((status != 0)); then
  {
    printf 'Building the macOS test executables failed (exit %s); the end of its log:\n' "$status"
    tail -n 60 "$scratch/build.err"
  } | tee /dev/stderr
  exit "$status"
fi
tail -n 20 "$scratch/build.err"

bundle="$scratch/bundle"
mkdir -p "$bundle/tests"
jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable' \
  "$build_json" | sort -u >"$scratch/tests.txt"
jq -r 'select(.reason == "compiler-artifact" and .profile.test == false and .target.name == "cv" and (.target.kind | index("bin")) and .executable != null) | .executable' \
  "$build_json" | sort -u >"$scratch/cv.txt"
[[ -s $scratch/tests.txt ]] || { echo 'The build produced no test executable.' >&2; exit 2; }
[[ $(wc -l <"$scratch/cv.txt") -eq 1 ]] || { echo 'The build did not produce exactly one cv binary for the tests.' >&2; exit 2; }
names=()
while IFS= read -r test_exe; do
  [[ $(ccvl_macho_arch "$test_exe") == x86_64 ]] || { echo "$test_exe is not an x86_64 Mach-O executable." | tee /dev/stderr; exit 2; }
  cp -- "$test_exe" "$bundle/tests/$(basename -- "$test_exe")"
  names+=("$(basename -- "$test_exe")")
done <"$scratch/tests.txt"
cp -- "$(cat "$scratch/cv.txt")" "$bundle/cv"
chmod 0755 "$bundle/cv" "$bundle"/tests/*
# What the clone checks before it runs anything: the bundle is of THIS commit and THIS runtime
# (the identity of the cross receipt that was written for the executable under test), and
# every file is the one that was bundled.
receipt="$output/cv-macos-x86_64.receipt.json"
[[ -s $receipt ]] || { echo "$receipt is missing: the cross stage must run before the test bundle." >&2; exit 2; }
runtime_id="$(jq -er '.runtime_id' "$receipt")"
source_commit="${CI_COMMIT_SHA:-$(git rev-parse HEAD)}"
(cd "$bundle" && sha256sum cv tests/*) | jq -R 'split("  ") | {(.[1]): .[0]}' | jq -s 'add' >"$scratch/files.json"
printf '%s\n' "${names[@]}" | jq -R . | jq -s --arg src "$src" --arg commit "$source_commit" --arg runtime "$runtime_id" \
  --slurpfile files "$scratch/files.json" \
  '{src: $src, source_commit: $commit, runtime_id: $runtime, files: $files[0], tests: .}' >"$bundle/suite.json"

mkdir -p "$output"
archive="$output/macos-tests-x86_64.tar.gz"
tar -czf "$archive.part" -C "$bundle" .
mv -f -- "$archive.part" "$archive"
(cd "$output" && sha256sum macos-tests-x86_64.tar.gz >macos-tests-x86_64.tar.gz.sha256)
printf '%s macOS test executables built in %s s: %s (%s bytes), source path %s.\n' \
  "${#names[@]}" "$((SECONDS - started))" "$archive" "$(wc -c <"$archive")" "$src"
