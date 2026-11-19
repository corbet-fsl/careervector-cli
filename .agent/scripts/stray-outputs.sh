#!/usr/bin/env bash
# Sourced, not run: what a build may leave in the source tree, and the one thing that is removed.
#
# cargo runs rustc, and so the linker, with the workspace root as its working directory,
# whatever the directory cargo itself was started in. The zig linker of the darwin builds
# writes `a.o` there when it is given no output name (release runs 109 and 111). The evidence
# refuses a dirty checkout, so the cross stage snapshots the tree before a build and, after it,
# removes exactly the new untracked `*.o` files directly in the root. Every other new entry is
# left alone and reported; a file that existed before the build is never touched.

# ccvl_tree_snapshot FILE: the porcelain status (untracked files listed one by one), sorted.
ccvl_tree_snapshot() {
  git status --porcelain --untracked-files=all | sort >"$1"
}

# ccvl_remove_stray_objects BEFORE [ROOT]: compare the tree with the snapshot BEFORE. Returns 0
# when nothing is new or when the only new entries were stray objects (now removed); returns 1,
# deleting nothing, when anything else is new.
ccvl_remove_stray_objects() {
  local before="$1" after new others name
  after="$(mktemp "${TMPDIR:-/tmp}/tree-after.XXXXXX")"
  ccvl_tree_snapshot "$after"
  new="$(comm -13 "$before" "$after")"
  rm -f "$after"
  [[ -n $new ]] || return 0
  # An entry is a stray object when it is untracked, in the root (no slash) and ends in .o.
  others="$(printf '%s\n' "$new" | grep -v -E '^\?\? [^/]+\.o$' || true)"
  if [[ -n $others ]]; then
    printf 'The build left more than stray linker output in the source tree:\n%s\n' "$others"
    return 1
  fi
  while IFS= read -r name; do
    name="${name#\?\? }"
    printf 'removing stray linker output: %s\n' "$name"
    rm -f -- "$name"
  done <<<"$new"
}
