#!/usr/bin/env bash
#
# Sync wasm-tools test fixtures into tests/wasm-tools/.
#
# Run this whenever you bump the wasmparser dep so the vendored fixtures
# match the upstream snapshot for that version.
#
# Usage:
#   scripts/sync-wasm-tools-fixtures.sh [--clone PATH] [--ref GIT_REF] [--dry-run] [--yes]
#
# Options:
#   --clone PATH    Path to a local wasm-tools git clone. Defaults to
#                   $WASM_TOOLS_CLONE if set, otherwise ../wasm-tools.
#   --ref GIT_REF   Git ref (tag/branch/SHA) to sync from. Overrides the
#                   version auto-detected from Cargo.toml.
#   --dry-run       Show what would change, touch nothing. Skips the prompt.
#   --yes, -y       Skip the confirmation prompt (useful in CI).
#
# Environment:
#   WASM_TOOLS_CLONE  Default value for --clone. Useful if your clone
#                     isn't side-by-side with wirm — set it in your
#                     shell profile so you don't have to pass --clone
#                     every time.
#
# Behaviour:
#   Auto-detects the target wasm-tools tag from the wasmparser version in
#   Cargo.toml (e.g. "0.245.0" -> "v1.245.0"), reads the currently-pinned
#   SHA from tests/wasm-tools/UPSTREAM-PIN, and prompts "bumping X -> Y,
#   proceed?" before touching anything.
#
#   Copies tests/cli/component-model/ and tests/cli/gc/ from the clone
#   into tests/wasm-tools/{component-model,gc}/ via rsync WITHOUT --delete.
#   wirm has some bespoke fixtures that aren't in upstream; those are
#   preserved and reported at the end of the run so reviewers know to
#   audit them if they look stale.
#
#   Writes tests/wasm-tools/UPSTREAM-PIN recording the resolved SHA so
#   code review can tell exactly which snapshot we're at.

set -euo pipefail

usage() {
    sed -n '3,38p' "${BASH_SOURCE[0]}" | sed 's|^# \?||'
}

CLONE="${WASM_TOOLS_CLONE:-../wasm-tools}"
REF=""
DRY_RUN=0
ASSUME_YES=0

while [ $# -gt 0 ]; do
    case "$1" in
        --clone)   CLONE="$2"; shift 2;;
        --ref)     REF="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        --yes|-y)  ASSUME_YES=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "error: unknown argument: $1" >&2; usage >&2; exit 2;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/tests/wasm-tools"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

# Resolve --clone to an absolute path. Preserve the raw user-facing value
# so error messages point at what they actually typed/defaulted to.
CLONE_RAW="$CLONE"
if ! CLONE="$(cd "$CLONE_RAW" 2>/dev/null && pwd)"; then
    echo "error: wasm-tools clone not found at '$CLONE_RAW'" >&2
    echo "" >&2
    echo "  Pass --clone PATH pointing at your local wasm-tools checkout, e.g." >&2
    echo "      $0 --clone \"\$HOME/git/research/wasm/wasm-tools\"" >&2
    echo "" >&2
    echo "  Or set WASM_TOOLS_CLONE in your shell profile so you don't have" >&2
    echo "  to pass it each time:" >&2
    echo "      export WASM_TOOLS_CLONE=\"\$HOME/git/research/wasm/wasm-tools\"" >&2
    exit 1
fi

if ! git -C "$CLONE" rev-parse --git-dir >/dev/null 2>&1; then
    echo "error: $CLONE is not a git repository" >&2
    exit 1
fi

SRC_CM="$CLONE/tests/cli/component-model"
SRC_GC="$CLONE/tests/cli/gc"
for d in "$SRC_CM" "$SRC_GC"; do
    if [ ! -d "$d" ]; then
        echo "error: expected subtree not found: $d" >&2
        echo "       is the clone at the right repository? (bytecodealliance/wasm-tools)" >&2
        exit 1
    fi
done

# --- Decide on the target git ref ----------------------------------------
#
# If the user passed --ref, honor it. Otherwise, parse the wasmparser
# version out of Cargo.toml and map it to the matching wasm-tools tag
# (wasmparser 0.X.Y <-> wasm-tools v1.X.Y — the crates release in lockstep).

AUTO_DETECTED=""
if [ -z "$REF" ]; then
    # Matches lines like:
    #   wasmparser = "0.245.0"
    #   wasmparser = { version = "0.245.0", features = [...] }
    WASMPARSER_VER="$(grep -E '^\s*wasmparser\s*=' "$CARGO_TOML" \
        | head -n1 \
        | grep -oE '"0\.[0-9]+\.[0-9]+"' \
        | tr -d '"' || true)"
    if [ -z "$WASMPARSER_VER" ]; then
        echo "error: could not auto-detect wasmparser version from Cargo.toml" >&2
        echo "       pass --ref <git-ref> explicitly" >&2
        exit 1
    fi
    REF="v1.${WASMPARSER_VER#0.}"
    AUTO_DETECTED="yes"
fi

# --- Figure out what we're going from vs. going to -----------------------

CURRENT_PIN="(no UPSTREAM-PIN file; first sync)"
if [ -f "$DEST/UPSTREAM-PIN" ]; then
    CURRENT_PIN="$(grep -E '^upstream_desc:' "$DEST/UPSTREAM-PIN" \
        | sed 's/^upstream_desc:[[:space:]]*//' || true)"
    [ -z "$CURRENT_PIN" ] && CURRENT_PIN="(UPSTREAM-PIN exists but is unparseable)"
fi

echo "==> wasm-tools clone:  $CLONE"
if [ -n "$AUTO_DETECTED" ]; then
    echo "==> target ref:        $REF  (auto-detected from Cargo.toml: wasmparser = \"$WASMPARSER_VER\")"
else
    echo "==> target ref:        $REF  (explicit --ref)"
fi
echo "==> current pin:       $CURRENT_PIN"
echo ""
echo "Plan: sync fixtures from $CURRENT_PIN -> $REF"

# --- Confirm --------------------------------------------------------------

if [ "$DRY_RUN" -eq 1 ]; then
    echo "(dry-run mode; no prompt)"
elif [ "$ASSUME_YES" -eq 1 ]; then
    echo "(--yes; skipping prompt)"
else
    printf "Proceed? [y/N] "
    read -r reply
    case "$reply" in
        y|Y|yes|YES) ;;
        *) echo "aborted."; exit 0;;
    esac
fi

# --- Do the work ---------------------------------------------------------

# Check out the requested ref. A dirty working tree in the clone will cause
# `git checkout` to fail, which is the behaviour we want — the user should
# commit or stash their own work first, not have the script mask it.
echo ""
echo "==> checking out $REF in $CLONE"
if [ "$DRY_RUN" -eq 0 ]; then
    git -C "$CLONE" fetch --tags origin
    git -C "$CLONE" checkout "$REF"
    RESOLVED_SHA="$(git -C "$CLONE" rev-parse HEAD)"
    RESOLVED_DESC="$(git -C "$CLONE" describe --always --tags HEAD 2>/dev/null || echo "$RESOLVED_SHA")"
else
    # In dry-run we don't actually check out, but we still want to report
    # what the sync *would* be from — resolve the target ref in place.
    RESOLVED_SHA="$(git -C "$CLONE" rev-parse "$REF" 2>/dev/null || echo "$REF")"
    RESOLVED_DESC="$REF"
fi

RSYNC_FLAGS=(-rt --itemize-changes)
if [ "$DRY_RUN" -eq 1 ]; then
    RSYNC_FLAGS+=(--dry-run)
fi

echo ""
echo "==> syncing component-model (from $RESOLVED_DESC)"
rsync "${RSYNC_FLAGS[@]}" "$SRC_CM/" "$DEST/component-model/"

echo ""
echo "==> syncing gc (from $RESOLVED_DESC)"
rsync "${RSYNC_FLAGS[@]}" "$SRC_GC/" "$DEST/gc/"

# Report files that exist locally but not upstream. These are preserved by
# the sync; the user should eyeball them to decide if any are stale renames
# (e.g. upstream renamed `foo.wast` -> `foo_bar.wast` and the old name
# lingers here).
echo ""
echo "==> files only in wirm (preserved; review if any look stale)"
any_local_only=0
for subdir in component-model gc; do
    local_only="$(comm -23 \
        <(cd "$DEST/$subdir" && find . -type f | sort) \
        <(cd "$CLONE/tests/cli/$subdir" && find . -type f | sort))"
    if [ -n "$local_only" ]; then
        any_local_only=1
        echo "  $subdir/:"
        # shellcheck disable=SC2001
        echo "$local_only" | sed 's|^\./|    |'
    fi
done
if [ "$any_local_only" -eq 0 ]; then
    echo "  (none)"
fi

# Write the pin file unless this is a dry run.
if [ "$DRY_RUN" -eq 0 ]; then
    cat > "$DEST/UPSTREAM-PIN" <<EOF
# Auto-generated by scripts/sync-wasm-tools-fixtures.sh. Do not edit by hand.
#
# Records which wasm-tools snapshot the vendored fixtures were last synced
# against. Files only in wirm (not present upstream at this SHA) are kept
# as-is by the sync script -- look for them in the script's output.
upstream_sha:  $RESOLVED_SHA
upstream_desc: $RESOLVED_DESC
synced_at:     $(date -u +"%Y-%m-%dT%H:%M:%SZ")
EOF
    echo ""
    echo "==> wrote $DEST/UPSTREAM-PIN"
fi

echo ""
echo "done"
