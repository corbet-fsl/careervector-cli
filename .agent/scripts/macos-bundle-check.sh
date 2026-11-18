#!/usr/bin/env bash
# Sourced, not run, in the macOS clone (bash 3.2, BSD userland): is the extracted test bundle
# exactly what suite.json lists?

# ccvl_bundle_complete DIR: DIR/suite.json lists every file of the bundle (`.files`: path to
# sha256) and the bundle holds nothing else. A file that is not listed, a listed file that is
# missing, and anything that is not a plain file (a link, a device) is refused, by name.
ccvl_bundle_complete() {
  local dir="$1" listed present extra missing odd
  [[ -f $dir/suite.json ]] || { echo "$dir/suite.json is missing." >&2; return 2; }
  odd="$(cd "$dir" && find . ! -type f ! -type d | sed 's#^\./##' | sort)"
  if [[ -n $odd ]]; then
    printf 'The test bundle holds something that is not a plain file: %s\n' "$(printf '%s' "$odd" | tr '\n' ' ')" >&2
    return 2
  fi
  listed="$(jq -r '.files | keys[]' "$dir/suite.json" | sort)"
  present="$(cd "$dir" && find . -type f | sed 's#^\./##' | grep -v '^suite.json$' | sort)"
  extra="$(comm -13 <(printf '%s\n' "$listed") <(printf '%s\n' "$present"))"
  missing="$(comm -23 <(printf '%s\n' "$listed") <(printf '%s\n' "$present"))"
  if [[ -n $extra ]]; then
    printf 'The test bundle holds files that suite.json does not list: %s\n' "$(printf '%s' "$extra" | tr '\n' ' ')" >&2
    return 2
  fi
  if [[ -n $missing ]]; then
    printf 'suite.json lists files that the test bundle does not hold: %s\n' "$(printf '%s' "$missing" | tr '\n' ' ')" >&2
    return 2
  fi
}
