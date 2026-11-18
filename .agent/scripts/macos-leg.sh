#!/usr/bin/env bash
# The macOS leg of a minor release: test the macOS executables in the macOS build clone and
# leave the evidence in the draft of the commit. The clone only TESTS.
#
#   macos-leg.sh        from the root of the checkout of the exact commit: the `release-macos` job of the
#                       rendered release workflow (`ccid release leg macos`, from [release] legs)
#                       checks it out and runs this as the leg's one check
#
# Both macOS platforms were built on Linux by the release job (cross-release.sh: cargo-zigbuild
# against the macOS SDK), together with the test executables of x86_64 (macos-test-bundle.sh).
# Their cross receipts and the test bundle are in the draft `build-<commit>`. This leg:
#
#   1. fetches cv-macos-x86_64 and the test bundle from the draft and checks them against the receipt;
#   2. puts the source tree of the commit (git archive, then the LFS objects) at the absolute
#      path the test executables were built from (suite.json records it);
#   3. runs the test executables there (RUST_TEST_THREADS=4: eight rebooted the 16 GiB guest),
#      driving the fetched executable's debug sibling (CCVL_TEST_BIN);
#   4. runs the public check and the runtime checks of the shipped executable;
#   5. writes the report and lets macos-attest.sh re-issue the x86_64 receipt as a native one.
#
# macos-arm64 is built only (ARM is never emulated): its cross receipt stays as the release job left it.
#
# No cargo, no rustup, no compiler and no git history are needed in the clone. It is a Mac of
# 2026: bash 3.2 (no mapfile, no associative arrays, no empty-array expansion under set -u), the
# BSD userland, no node. The job owns the clone for its length and the clone is rolled back afterwards.
set +x
set -euo pipefail
repo_root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

stage() { printf '\n== %s (%s s into the leg)\n' "$1" "$SECONDS"; }
refuse() { printf '%s\n' "$1" >&2; exit "${2:-2}"; }
sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
platform=macos-x86_64
host=x86_64-apple-darwin

# 1. This must be the macOS clone, on a Forgejo run of public ccvl main.
stage 'The clone'
[[ $(uname -s) == Darwin && $(uname -m) == x86_64 ]] || refuse 'The macOS leg runs in the macOS build clone (Darwin, x86_64).'
[[ $(sysctl -in sysctl.proc_translated 2>/dev/null || true) != 1 ]] || refuse 'A translated macOS process can not provide native evidence.'
[[ -z ${GH_TOKEN:-} ]] || refuse 'Publication credentials must not reach the macOS leg.'
[[ ${FORGEJO_ACTIONS:-} == true && ${GITHUB_REPOSITORY:-} == corbet-fsl/careervector-cli && ${GITHUB_REF_NAME:-} == main ]] \
  || refuse 'The macOS leg requires a Forgejo run on public ccvl main.'
for tool in jq curl git tar cmp shasum awk; do
  command -v "$tool" >/dev/null || refuse "Missing tool: $tool (the macOS clone provides it)."
done
sha="${GITHUB_SHA:-}"
[[ $sha =~ ^[0-9a-f]{40}$ ]] || refuse 'The exact source commit (GITHUB_SHA) is required.'
export CI_COMMIT_SHA="$sha"
[[ $(git rev-parse HEAD) == "$sha" ]] || refuse "The checkout is not the commit of this run ($sha)."
printf 'macOS %s, %s, commit %s\n' "$(sw_vers -productVersion)" "$(uname -m)" "$sha"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/ccvl-macos-leg.XXXXXXXX")"
trap 'find "$scratch" -delete' EXIT

# 2. What the release holds, before anything is fetched: a leg for a release without macOS ends here.
# (`ccid release leg macos` has checked that the draft of this commit is there and is main's.)
stage 'The release'
: "${CCID_BIN:?the ccid that runs this leg}"
version="${CCID_RELEASE_VERSION:?}"
kind="${CCID_RELEASE_CLASS:?}"
if ! printf '%s\n' "${CCID_RELEASE_PLATFORMS:-}" | tr ' ' '\n' | grep -Fxq "$platform"; then
  printf 'ccvl %s is a %s release: it holds no macOS platform, nothing to do.\n' "$version" "$kind"
  exit 0
fi
printf 'ccvl %s, a %s release; the draft is build-%s.\n' "$version" "$kind" "${sha:0:12}"

# 3. What the release job built, from the draft; the receipt vouches for the executable.
stage 'The draft: fetch'
dist="$scratch/dist"
mkdir -p "$dist"
"$CCID_BIN" release fetch "cv-$platform" "cv-$platform.sha256" "cv-$platform.runtime-id" \
  "cv-$platform.receipt.json" macos-tests-x86_64.tar.gz macos-tests-x86_64.tar.gz.sha256 --into "$dist"
binary="$dist/cv-$platform"
(cd "$dist" && shasum -a 256 -c "cv-$platform.sha256" && shasum -a 256 -c macos-tests-x86_64.tar.gz.sha256) \
  || refuse 'The fetched files do not match their checksums.'
jq -e --arg p "$platform" --arg h "$host" '.kind == "cross" and .platform == $p and .rust_host == $h and .tested == false' \
  "$dist/cv-$platform.receipt.json" >/dev/null || refuse "The draft holds no cross receipt of $platform to test."
receipt_sha="$(jq -er --arg n "cv-$platform" '.assets[$n]' "$dist/cv-$platform.receipt.json")"
exe_sha="$(sha256 "$binary")"
[[ $receipt_sha == "$exe_sha" ]] || refuse "The executable does not match its receipt: receipt $receipt_sha, bytes $exe_sha."
runtime_id="$(jq -er '.runtime_id' "$dist/cv-$platform.receipt.json")"
chmod 0755 "$binary"
mkdir -p "$scratch/bundle"
tar -xzf "$dist/macos-tests-x86_64.tar.gz" -C "$scratch/bundle"
# The one fixed root both sides can create without root (macos-sdk-env.sh); the bundle records it.
# shellcheck source=.agent/scripts/macos-sdk-env.sh
source .agent/scripts/macos-sdk-env.sh
src="$CCVL_MACOS_TEST_SRC"
[[ $(jq -er '.src' "$scratch/bundle/suite.json") == "$src" ]] \
  || refuse "suite.json names another source path than $src: the bundle was built for a different layout."
# The bundle is of this commit and this runtime, and its files are the bundled ones; the names
# it asks to run are plain file names of the bundle, never paths.
[[ $(jq -er '.source_commit' "$scratch/bundle/suite.json") == "$sha" ]] \
  || refuse 'The test bundle was built from another commit than this run.'
[[ $(jq -er '.runtime_id' "$scratch/bundle/suite.json") == "$runtime_id" ]] \
  || refuse 'The test bundle was built for another runtime identity than the executable under test.'
while IFS=' ' read -r bundled_sum bundled_path; do
  [[ $(sha256 "$scratch/bundle/$bundled_path") == "$bundled_sum" ]] \
    || refuse "$bundled_path in the test bundle differs from the file that was bundled."
done < <(jq -r '.files | to_entries[] | "\(.value) \(.key)"' "$scratch/bundle/suite.json")
while IFS= read -r test_name; do
  [[ $test_name =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]*$ && -f $scratch/bundle/tests/$test_name ]] \
    || refuse "The test bundle names an unusable test executable: $test_name"
done < <(jq -r '.tests[]' "$scratch/bundle/suite.json")
tests_total="$(jq -er '.tests | length' "$scratch/bundle/suite.json")"
printf 'cv-%s sha256 %s; %s test executables for the source at %s\n' "$platform" "$exe_sha" "$tests_total" "$src"

# 4. The source: the committed pointers are the archive (as the Linux run stages it), the LFS
# objects they name are in the checkout. Both the evidence command and the tests use it.
stage 'The source'
git archive "$sha" > "$scratch/source.tar"
export SOURCE_ARCHIVE="$scratch/source.tar"
SOURCE_SHA256="$(sha256 "$SOURCE_ARCHIVE")"
export SOURCE_SHA256
bash .agent/scripts/lfs-fetch.sh .
# The tree at the path the test executables embed. A directory this leg did not make is not
# touched: the clone is rolled back after every job, so it is normally absent.
# A directory this leg did not make is not touched; the clone is rolled back after every
# job, so the path is normally absent.
if [[ -e $src ]]; then
  [[ -e $CCVL_MACOS_TEST_SRC_MARKER ]] || refuse "$src exists and was not made by this leg; refusing to write over it."
  rm -rf -- "$src"
fi
mkdir -p "$src" || refuse "cannot create $src, where the test executables expect the source tree."
: > "$CCVL_MACOS_TEST_SRC_MARKER"
tar -cf - --exclude=.git -C "$repo_root" . | tar -xf - -C "$src"

# 5. The test executables, in the clone: the whole suite built on Linux, run here. The
# publication scripts and their fixtures need bash 4 and GNU tools and are Linux-only (the
# tests say so).
stage 'The Rust test suite'
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
export CCVL_TEST_BIN="$scratch/bundle/cv"
tests_failed=0
passed=0
failed=0
ignored=0
executables=0
while IFS= read -r name; do
  executables=$((executables + 1))
  log="$scratch/test-$name.log"
  code=0
  (cd "$src" && "$scratch/bundle/tests/$name") >"$log" 2>&1 || code=$?
  counts="$(awk '/^test result:/ { p += $4; f += $6; i += $8 } END { print p + 0, f + 0, i + 0 }' "$log")"
  # shellcheck disable=SC2086 # three numbers, split on purpose
  set -- $counts
  passed=$((passed + $1))
  failed=$((failed + $2))
  ignored=$((ignored + $3))
  printf '%s: exit %s, %s passed, %s failed, %s ignored\n' "$name" "$code" "$1" "$2" "$3"
  if [[ $code != 0 ]]; then
    tests_failed=$((tests_failed + 1))
    grep -E '^    [A-Za-z_0-9:]+$|panicked at|^error' "$log" | sort -u | head -n 40 >&2 || true
  fi
done < <(jq -r '.tests[]' "$scratch/bundle/suite.json")
suite_ok=false
if [[ $tests_failed == 0 && $failed == 0 && $passed -gt 0 && $executables == "$tests_total" ]]; then suite_ok=true; fi
printf 'macOS test suite: %s passed, %s failed, %s ignored in %s executables.\n' "$passed" "$failed" "$ignored" "$executables"

# 6. The shipped executable itself: what it is, the public check on this source, the runtime checks.
stage 'The executable'
checks="$scratch/checks.ndjson"
: > "$checks"
add_check() { # id, name, ok(true|false), detail
  jq -n -c --arg id "$1" --arg name "$2" --argjson ok "$3" --arg detail "$4" '{id: $id, name: $name, ok: $ok, detail: $detail}' >> "$checks"
}
add_check sha256 'the executable that runs is the release asset (sha256)' true "$exe_sha"
arch_detail="$(file -b "$binary" 2>/dev/null || true)"
arch_ok=false
case "$arch_detail" in *x86_64*) arch_ok=true ;; esac
add_check architecture 'the executable is x86_64 macOS' "$arch_ok" "$(uname -m): $arch_detail"
version_out="$("$binary" --version 2>&1)" && version_ok=true || version_ok=false
add_check version 'the executable starts (--version)' "$version_ok" "$version_out"
runtime_out="$("$binary" runtime-id 2>&1)" || true
runtime_ok=false
[[ $runtime_out == "$runtime_id" ]] && runtime_ok=true
add_check runtime-id 'the identity compiled into it is the release identity (runtime-id)' "$runtime_ok" "$runtime_out"
public_ok=true
"$binary" public-check >"$scratch/public-check.log" 2>&1 || public_ok=false
add_check public-check 'public-check on the source of the commit (every document rendered and verified)' "$public_ok" "$(tail -n 1 "$scratch/public-check.log")"
runtime_checks=true
bash .agent/tests/test_runtime.sh "$binary" >"$scratch/test-runtime.log" 2>&1 || runtime_checks=false
printf 'public-check: %s; runtime checks: %s\n' "$public_ok" "$runtime_checks"
add_check test-suite 'the workspace test suite built for the same ABI' "$([[ $suite_ok == true && $runtime_checks == true ]] && echo true || echo false)" \
  "$passed passed, $failed failed, $ignored ignored in $executables test executables; runtime checks $runtime_checks"
report="$scratch/report.json"
jq -n --arg commit "$sha" --arg platform "$platform" --arg host "$host" --arg sha "$exe_sha" --arg runtime "$runtime_id" \
  --argjson executables "$executables" --argjson passed "$passed" --argjson failed "$failed" --argjson ignored "$ignored" \
  --slurpfile checks "$checks" \
  '{schema: 1, platform: $platform, rust_host: $host, source_commit: $commit, executable_sha256: $sha, runtime_id: $runtime,
    tests: {executables: $executables, passed: $passed, failed: $failed, ignored: $ignored}, checks: $checks}' > "$report"
jq -r '.checks[] | "\(if .ok then "ok  " else "FAIL" end) \(.name): \(.detail)"' "$report"
jq -e '[.checks[].ok] | all' "$report" >/dev/null || refuse 'The macOS clone did not pass every check; nothing is attested.' 1

# 7. The receipt of x86_64 becomes a native one; the clone's report is recorded beside it.
stage 'The draft: attest'
export CCVL_RELEASE_EVIDENCE_BIN="$binary"
bash .agent/scripts/macos-attest.sh "$report"
