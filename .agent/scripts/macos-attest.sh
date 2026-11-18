#!/usr/bin/env bash
# Make the macOS test count: when the shipped macOS x86_64 executable and the test suite
# passed in the macOS clone (macos-leg.sh), the cross-build receipt in the draft of the
# release becomes a NATIVE receipt, and the manifests say `tested`. The pattern is
# windows-attest.sh's; this one runs IN the clone (bash 3.2, BSD userland) and uses the
# macOS executable of the release as the evidence command.
#
#   macos-attest.sh REPORT.json      REPORT.json is what macos-leg.sh wrote
#
# It reads the one draft of this commit (`ccid release fetch`), lets `cv release-evidence attest`
# re-issue the receipt (it checks the report against the receipt: the very same executable,
# the same source, every check and the whole suite passed), lets `release-evidence check`
# write the manifests as promotion will, and replaces the changed files in the draft:
#   cv-macos-x86_64.receipt.json        the native receipt
#   cv-macos-x86_64.native-result.json  the clone's report, as recorded by the receipt
#   cv-macos-x86_64.cross-receipt.json  the receipt it replaces (its checksum is in the new one)
#   manifest.json, runtime-manifest.json
# Nothing else is touched and nothing is published. Run twice, it changes nothing the second time.
# SOURCE_ARCHIVE and SOURCE_SHA256 (macos-leg.sh) name the verified source for the evidence command.
set +x
set -euo pipefail
repo_root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
report="${1:?Usage: macos-attest.sh REPORT.json}"
[[ -s $report ]] || { echo "No report: $report" >&2; exit 2; }
platform=macos-x86_64
: "${CCID_BIN:?Missing the ccid that runs this leg}"
: "${CI_COMMIT_SHA:?Missing the source commit}"
sha="$CI_COMMIT_SHA"
[[ -z ${GH_TOKEN:-} ]] || { echo 'Publication credentials must not reach the macOS attestation.' >&2; exit 2; }
for tool in jq shasum cmp; do
  command -v "$tool" >/dev/null || { echo "Missing tool: $tool" >&2; exit 2; }
done
scratch="$(mktemp -d "${TMPDIR:-/tmp}/ccvl-macos-attest.XXXXXXXX")"
trap 'find "$scratch" -delete' EXIT

dist="$scratch/dist"
mkdir -p "$dist"
# Every asset of the draft (the runner's read credential reads a draft; the job token cannot).
"$CCID_BIN" release fetch --into "$dist"
receipt_name="cv-$platform.receipt.json"
cross_name="cv-$platform.cross-receipt.json"
# A run that died between removing the old receipt and uploading the new one: the copy
# of the old one is on the release.
if [[ ! -e $dist/$receipt_name && -e $dist/$cross_name ]]; then
  echo "The release has no $receipt_name; starting from $cross_name."
  cp -- "$dist/$cross_name" "$dist/$receipt_name"
fi
runtime_id="$(jq -er '.runtime_id' "$dist/$receipt_name")"
[[ "$runtime_id" =~ ^[0-9a-f]{64}$ ]] || { echo 'The receipt carries no runtime identity.' >&2; exit 2; }

# The evidence command is the macOS executable of this very release (it runs here and carries
# the identity of this source), unless the environment names one.
if [[ -z ${CCVL_RELEASE_EVIDENCE_BIN:-} ]]; then
  mac_exe="$dist/cv-$platform"
  (cd "$dist" && shasum -a 256 -c "cv-$platform.sha256" >/dev/null)
  chmod 0755 "$mac_exe"
  [[ $(cd "$repo_root" && "$mac_exe" runtime-id) == "$runtime_id" ]] || {
    echo 'The macOS executable of the release does not carry the runtime identity of this source.' >&2; exit 2
  }
  export CCVL_RELEASE_EVIDENCE_BIN="$mac_exe"
fi
# shellcheck source=.agent/scripts/release-evidence.sh
source .agent/scripts/release-evidence.sh
ccvl_prepare_release_evidence

kind="$(jq -r '.kind' "$dist/$receipt_name")"
if [[ $kind == cross ]]; then
  cp -- "$dist/$receipt_name" "$dist/$cross_name"
  "${CCVL_RELEASE_EVIDENCE_COMMAND[@]}" attest --platform "$platform" --result "$report" --output "$dist"
else
  echo "The receipt of $platform is already native (run $(jq -r '.run' "$dist/$receipt_name")); verifying it."
fi
# The manifests are derived from the receipts: write them again, as `check` does at promotion.
find "$dist/manifest.json" "$dist/runtime-manifest.json" -delete 2>/dev/null || true
"${CCVL_RELEASE_EVIDENCE_COMMAND[@]}" check --output "$dist" >/dev/null
for file in manifest.json runtime-manifest.json; do
  [[ $(jq -r --arg p "$platform" '.platform_states[$p]' "$dist/$file") == tested ]] || {
    echo "$file does not say that $platform is tested." >&2; exit 2
  }
done

# put NAME: the draft ends up holding $dist/NAME under NAME, replacing what differs (a draft only).
put() { "$CCID_BIN" release put "$dist/$1"; }
echo "Draft of $sha:"
# First the new names, then the receipt (the old one is kept as $cross_name), then the
# manifests that describe them.
[[ ! -e $dist/$cross_name ]] || put "$cross_name"
put "cv-$platform.native-result.json"
put "$receipt_name"
put manifest.json
put runtime-manifest.json

# What the forge holds now is what promotion will check: read it back.
for file in manifest.json runtime-manifest.json "$receipt_name"; do
  "$CCID_BIN" release fetch "$file" --into "$scratch/read-back"
  cmp -s -- "$scratch/read-back/$file" "$dist/$file" || { echo "$file on the forge differs from the checked one." >&2; exit 2; }
done
for file in manifest.json runtime-manifest.json; do
  jq -e --arg p "$platform" '.platform_states[$p] == "tested"' "$dist/$file" >/dev/null
done
printf 'Platform states in the draft: %s\n' "$(jq -c '.platform_states' "$dist/manifest.json")"
printf '%s is tested: the shipped executable and the whole test suite passed in the macOS clone (run %s).\n' \
  "$platform" "$(jq -r '.run' "$dist/$receipt_name")"
