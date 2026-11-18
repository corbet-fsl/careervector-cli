#!/usr/bin/env bash
# Sourced, not run: the claim on the fixed source root of the macOS test bundle
# (macos-sdk-env.sh names it) on the Linux build host.
#
# /var/tmp is shared and persistent there, and runs are only serialized in CI. So:
#   - the root is a real directory that we own (never a symbolic link, never someone else's),
#     created 0700 when missing;
#   - one `flock` on the root is held for the whole build; a second run is refused;
#   - the tree is made only when the path is absent, or is the leftover of an earlier run (it
#     carries our marker, and nobody else holds the lock to be using it);
#   - the marker holds a random token, and the tree is removed at the end only if the marker
#     still holds OUR token. The cleanup is armed only after the claim succeeded: a refusal
#     deletes nothing.

# ccvl_xtest_claim: take the lock and make the empty tree. Returns 2 (nothing touched) if it can not.
ccvl_xtest_claim() {
  local root="$CCVL_MACOS_TEST_ROOT" src="$CCVL_MACOS_TEST_SRC" marker="$CCVL_MACOS_TEST_SRC_MARKER"
  if [[ -L $root ]]; then
    echo "$root is a symbolic link; refusing to build there." >&2
    return 2
  fi
  if [[ ! -e $root ]]; then
    mkdir -m 0700 -- "$root" || { echo "cannot create $root." >&2; return 2; }
  fi
  if [[ ! -d $root || -L $root || ! -O $root ]]; then
    echo "$root is not a directory of ours; refusing to build there." >&2
    return 2
  fi
  exec 9>"$root/lock" || { echo "cannot open the lock in $root." >&2; return 2; }
  flock -n 9 || { echo "another macOS test build holds $root/lock; refusing to run alongside it." >&2; return 2; }
  if [[ -L $src || -L $marker ]]; then
    echo "$src or its marker is a symbolic link; refusing." >&2
    return 2
  fi
  if [[ -e $src ]]; then
    if [[ ! -f $marker ]]; then
      echo "$src exists and was not made by this script; refusing to write over it." >&2
      return 2
    fi
    # The leftover of an earlier run that died: we hold the lock, so nobody is using it.
    rm -rf -- "$src" "$marker"
  fi
  mkdir -- "$src" || { echo "cannot create $src." >&2; return 2; }
  CCVL_XTEST_TOKEN="$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  printf '%s\n' "$CCVL_XTEST_TOKEN" >"$marker"
}

# ccvl_xtest_release: remove the tree and the marker if, and only if, the marker is still ours.
ccvl_xtest_release() {
  local marker="$CCVL_MACOS_TEST_SRC_MARKER"
  [[ -n ${CCVL_XTEST_TOKEN:-} && -f $marker && ! -L $marker ]] || return 0
  [[ "$(cat -- "$marker")" == "$CCVL_XTEST_TOKEN" ]] || return 0
  rm -rf -- "$CCVL_MACOS_TEST_SRC" "$marker"
}
