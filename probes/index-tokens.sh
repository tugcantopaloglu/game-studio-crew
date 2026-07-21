#!/usr/bin/env bash
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
OUT="$HERE/out"
MODEL="${PROBE_MODEL:-opus}"
SCRIPTS="${SCRIPTS:-60}"

if [ -n "${CLAUDECODE:-}${CLAUDE_CODE_CHILD_SESSION:-}" ] && [ -z "${PROBE_FORCE:-}" ]; then
  echo "REFUSING TO RUN: this shell is inside a Claude Code session."
  echo
  echo "A nested 'claude' does not inherit credentials and both arms will fail"
  echo "with 'Not logged in', producing a meaningless comparison."
  echo
  echo "Open a separate terminal, cd to the repo, and run this script there."
  echo "Using the '!' prefix inside Claude Code is NOT sufficient."
  exit 2
fi

STUDIOD="$REPO/target/release/studiod"
[ -x "$STUDIOD" ] || STUDIOD="$STUDIOD.exe"
if [ ! -x "$STUDIOD" ]; then
  echo "build first: cargo build --release -p studiod" >&2
  exit 1
fi

command -v claude >/dev/null || { echo "claude CLI not on PATH" >&2; exit 1; }

mkdir -p "$OUT"
FIXTURE="$OUT/token-fixture"
rm -rf "$FIXTURE"
mkdir -p "$FIXTURE/scripts" "$FIXTURE/scenes"

echo "probe model:  $MODEL"
echo "fixture:      $FIXTURE ($SCRIPTS scripts)"
echo

printf '[application]\nconfig/name="TokenProbe"\n' > "$FIXTURE/project.godot"

cat > "$FIXTURE/scripts/player.gd" <<'EOF'
class_name Player
extends CharacterBody2D

## emitted once the player runs out of health
signal died(cause: String)

const MAX_HEALTH := 100

@export var health: int = MAX_HEALTH

## reduces health and emits died at zero
func take_damage(amount: int, source: String) -> bool:
	health -= amount
	if health <= 0:
		died.emit(source)
		return true
	return false
EOF

cat > "$FIXTURE/scripts/enemy.gd" <<'EOF'
class_name Enemy
extends Node2D

@export var power: int = 12

func attack(target: Player) -> void:
	target.take_damage(power, "enemy")
EOF

cat > "$FIXTURE/scenes/main.tscn" <<'EOF'
[gd_scene load_steps=3 format=3 uid="uid://tokenprobe1"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]
[ext_resource type="Script" path="res://scripts/enemy.gd" id="2_e"]

[node name="Main" type="Node2D"]

[node name="Hero" type="CharacterBody2D" parent="."]
script = ExtResource("1_p")

[node name="Grunt" type="Node2D" parent="."]
script = ExtResource("2_e")
EOF

for i in $(seq 1 "$SCRIPTS"); do
  cat > "$FIXTURE/scripts/system_$i.gd" <<EOF
class_name System$i
extends Node

## subsystem $i, unrelated to combat
signal ready_changed(value: bool)

const TICK_RATE := 60.0

@export var enabled: bool = true
var _accumulated: float = 0.0

func _process(delta: float) -> void:
	_accumulated += delta
	if _accumulated > 1.0 / TICK_RATE:
		_accumulated = 0.0
		_step()

func _step() -> void:
	if not enabled:
		return
	ready_changed.emit(enabled)

func configure(value: bool) -> void:
	enabled = value
	ready_changed.emit(enabled)
EOF
done

echo "building the index"
( cd "$FIXTURE" && "$STUDIOD" index . )
echo

CHARTER="$OUT/token-charter.txt"
cat > "$CHARTER" <<'EOF'
You are gameplay_engineer#1 in a game studio.

Answer the question you are given using the tools available to you.
Be brief. Give the answer only, with no preamble and no restatement.
EOF

QUESTION='In this Godot project: Enemy.attack calls a method on its target. Give (1) that method'"'"'s exact signature, (2) the file and line where it is defined, and (3) the name and type of the scene node that mounts the script defining it. One line, no preamble.'

if command -v cygpath >/dev/null; then
  STUDIOD_NATIVE="$(cygpath -m "$STUDIOD")"
else
  STUDIOD_NATIVE="$STUDIOD"
fi

node -e '
const fs = require("fs");
fs.writeFileSync(process.argv[1], JSON.stringify({
  mcpServers: {
    studio: {
      command: process.argv[2],
      args: ["mcp-server", "--role", "gameplay_engineer", "--task", "probe",
             "--escalates-to", "systems_engineer"],
    },
  },
}, null, 2));
' "$OUT/token-mcp.json" "$STUDIOD_NATIVE"

COMMON=(-p
  --setting-sources ""
  --system-prompt-file "$CHARTER"
  --model "$MODEL"
  --effort low
  --permission-mode dontAsk
  --output-format stream-json
  --include-partial-messages
  --verbose)

echo "== arm A: index only (symbol_lookup, no file access) =="
( cd "$FIXTURE" && printf '%s' "$QUESTION" | claude "${COMMON[@]}" \
    --tools "" \
    --allowedTools "mcp__studio__symbol_lookup" \
    --mcp-config "$OUT/token-mcp.json" \
    --strict-mcp-config \
    > "$OUT/tokens-indexed.ndjson" 2>"$OUT/tokens-indexed.err" )
echo "  exit $?"

echo "== arm B: files only (Read, Grep, Glob; no index) =="
( cd "$FIXTURE" && printf '%s' "$QUESTION" | claude "${COMMON[@]}" \
    --tools "Read,Grep,Glob" \
    > "$OUT/tokens-files.ndjson" 2>"$OUT/tokens-files.err" )
echo "  exit $?"

echo
node "$HERE/index-tokens.js" "$OUT/tokens-indexed.ndjson" "$OUT/tokens-files.ndjson"
