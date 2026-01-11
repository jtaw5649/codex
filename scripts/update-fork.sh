#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UPSTREAM_URL="https://github.com/openai/codex.git"
ALLOW_DIRTY=0
AUTO_STASH=0
STASH_REF=""
STASH_NAME=""
INSTALL=0
RUN_CLIPPY=0
TESTS="hooks"

usage() {
  cat <<'USAGE'
Usage: scripts/update-fork.sh [options]

Options:
  --tests {hooks|full|none}  Which tests to run (default: hooks)
  --clippy                   Run cargo clippy --all-features --tests
  --install                  Install the local codex binary (default: off)
  --no-install               Skip installing the local codex binary
  --allow-dirty              Proceed without stashing a dirty working tree
  --upstream-url URL         Override upstream repo URL
  -h, --help                 Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tests)
      shift
      TESTS="${1:-}"
      ;;
    --clippy)
      RUN_CLIPPY=1
      ;;
    --install)
      INSTALL=1
      ;;
    --no-install)
      INSTALL=0
      ;;
    --allow-dirty)
      ALLOW_DIRTY=1
      ;;
    --upstream-url)
      shift
      UPSTREAM_URL="${1:-}"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

cd "$ROOT"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required but not installed" >&2
  exit 1
fi

PYTHON_BIN="python3"
if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  PYTHON_BIN="python"
fi
if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  echo "python3 (or python) is required to update the workspace version" >&2
  exit 1
fi

if ! git diff-index --quiet HEAD --; then
  if [[ "$ALLOW_DIRTY" -eq 1 ]]; then
    echo "Working tree is dirty. Proceeding without stashing per --allow-dirty."
  else
    AUTO_STASH=1
    STASH_NAME="jtaw-update-$(date +%Y%m%d-%H%M%S)"
    git stash push -u -m "$STASH_NAME"
    STASH_REF="$(git stash list -n 1 --pretty=format:%gd)"
    echo "Stashed local changes to ${STASH_REF} (${STASH_NAME})."
  fi
fi

if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream "$UPSTREAM_URL"
fi

git fetch upstream --tags
git rebase upstream/main

if [[ "$AUTO_STASH" -eq 1 ]]; then
  if git stash apply "${STASH_REF}"; then
    echo "Reapplied stashed changes from ${STASH_REF} (kept)."
  else
    echo "Failed to reapply ${STASH_REF}. Resolve conflicts, then run: git stash apply ${STASH_REF}" >&2
    exit 1
  fi
fi

update_workspace_version() {
  local version="$1"
  local cargo_toml="$ROOT/codex-rs/Cargo.toml"
  "$PYTHON_BIN" - "$cargo_toml" "$version" <<'PY'
import re
import sys

path, version = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as handle:
    lines = handle.read().splitlines()

out = []
in_section = False
updated = False

for line in lines:
    stripped = line.strip()
    if stripped.startswith("[") and stripped.endswith("]"):
        in_section = stripped == "[workspace.package]"
    if in_section and not updated and re.match(r"^\s*version\s*=\s*\"", line):
        match = re.match(r'(^\s*version\s*=\s*")([^"]+)(".*$)', line)
        if match:
            line = f"{match.group(1)}{version}{match.group(3)}"
            updated = True
    out.append(line)

if not updated:
    raise SystemExit("Failed to update [workspace.package] version in Cargo.toml")

with open(path, "w", encoding="utf-8") as handle:
    handle.write("\n".join(out) + "\n")
PY
}

pick_latest_tag() {
  local regex="$1"
  local tag
  while IFS= read -r tag; do
    if [[ "$tag" =~ $regex ]]; then
      echo "$tag"
      return 0
    fi
  done <<< "$LATEST_TAGS"
  return 1
}

LATEST_TAGS="$(git tag -l 'rust-v*' --sort=-v:refname)"
LATEST_TAG="$(pick_latest_tag '^rust-v[0-9]+\.[0-9]+\.[0-9]+$' || true)"
if [[ -z "$LATEST_TAG" ]]; then
  LATEST_TAG="$(pick_latest_tag '^rust-v[0-9]+\.[0-9]+\.[0-9]+-.+$' || true)"
fi
if [[ -z "$LATEST_TAG" ]]; then
  LATEST_TAGS="$(git tag -l 'v[0-9]*' --sort=-v:refname)"
  LATEST_TAG="$(pick_latest_tag '^v[0-9]+\.[0-9]+\.[0-9]+$' || true)"
  if [[ -z "$LATEST_TAG" ]]; then
    LATEST_TAG="$(pick_latest_tag '^v[0-9]+\.[0-9]+\.[0-9]+-.+$' || true)"
  fi
fi

if [[ -n "$LATEST_TAG" ]]; then
  VERSION="${LATEST_TAG#rust-v}"
  VERSION="${VERSION#v}"
  if [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z\.-]+)?$ ]]; then
    update_workspace_version "$VERSION"
    echo "Updated workspace version to ${VERSION} (from ${LATEST_TAG})."
  else
    echo "Failed to parse a valid version from ${LATEST_TAG}; skipping workspace version update." >&2
  fi
else
  echo "No version tags found; skipping workspace version update."
fi

case "$TESTS" in
  hooks)
    (cd codex-rs && cargo test -p codex-core hooks_)
    ;;
  full)
    if command -v cargo-nextest >/dev/null 2>&1; then
      (cd codex-rs && cargo nextest run --no-fail-fast)
    else
      (cd codex-rs && cargo test --all-features)
    fi
    ;;
  none)
    ;;
  *)
    echo "Unknown --tests value: $TESTS" >&2
    usage
    exit 1
    ;;
esac

if [[ "$RUN_CLIPPY" -eq 1 ]]; then
  (cd codex-rs && cargo clippy --all-features --tests)
fi

if [[ "$INSTALL" -eq 1 ]]; then
  cargo install --path codex-rs/cli --bin codex --locked --force
fi

echo "Update complete."
