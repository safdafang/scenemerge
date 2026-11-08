#!/usr/bin/env bash
# Usage: GODOT=/path/to/godot tests/godot/run.sh [GODOT_DEMO_PROJECTS_DIR]
set -euo pipefail

: "${GODOT:?set GODOT to the Godot 4 executable}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="${WORK:-$(mktemp -d)}"
SCENEMERGE="${SCENEMERGE:-$ROOT/target/release/scenemerge}"
PY="${PYTHON:-python3}"
DEMOS=("2d/dodge_the_creeps" "2d/platformer" "3d/platformer" "gui/control_gallery" "2d/role_playing_game")

check() {
  local project="$1" dir="$2"
  cp "$ROOT/tests/godot/check_load.gd" "$project/"
  "$GODOT" --headless --path "$project" --import >"$WORK/import.log" 2>&1 || true
  "$GODOT" --headless --path "$project" -s res://check_load.gd -- "$dir"
}

echo "== merge cases"
"$PY" "$ROOT/tests/godot/prepare_project.py" "$WORK/cases" --cases "$ROOT/tests/merge-cases"
check "$WORK/cases" res://merged

if [[ $# -ge 1 ]]; then
  for demo in "${DEMOS[@]}"; do
    [[ -d "$1/$demo" ]] || { echo "skip $demo (not found)"; continue; }
    echo "== $demo"
    target="$WORK/demo-$(echo "$demo" | tr '/' '-')"
    "$PY" "$ROOT/tests/godot/merge_demo_project.py" "$SCENEMERGE" "$1/$demo" "$target"
    check "$target" res://
  done
fi
echo "all Godot checks passed"
