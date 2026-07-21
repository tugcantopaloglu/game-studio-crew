#!/usr/bin/env bash
set -euo pipefail

MODULES="${MODULES:-40}"
UNITS="${UNITS:-50}"
ROOT="${ROOT:-$(mktemp -d)/index-scan}"
BIN="${BIN:-target/release/studiod}"

if [ ! -x "$BIN" ] && [ ! -x "$BIN.exe" ]; then
  echo "build first: cargo build --release -p studiod" >&2
  exit 1
fi
[ -x "$BIN" ] || BIN="$BIN.exe"
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

echo "generating $((MODULES * UNITS * 2 + 1)) files under $ROOT"
mkdir -p "$ROOT"
printf '[application]\nconfig/name="ScanProbe"\n' > "$ROOT/project.godot"

for d in $(seq 1 "$MODULES"); do
  mkdir -p "$ROOT/scripts/mod$d" "$ROOT/scenes/mod$d"
  for f in $(seq 1 "$UNITS"); do
    cat > "$ROOT/scripts/mod$d/unit_$f.gd" <<EOF
class_name Unit_${d}_${f}
extends Node2D

## unit $d $f
signal changed(value: int)

var hp: int = 100

func apply(amount: int) -> void:
	hp -= amount
	changed.emit(hp)

func reset() -> void:
	hp = 100
EOF
    cat > "$ROOT/scenes/mod$d/unit_$f.tscn" <<EOF
[gd_scene load_steps=2 format=3 uid="uid://probe${d}x${f}"]

[ext_resource type="Script" path="res://scripts/mod$d/unit_$f.gd" id="1_u"]

[node name="Unit" type="Node2D"]
script = ExtResource("1_u")

[node name="Sprite" type="Sprite2D" parent="."]
EOF
  done
done

cd "$ROOT"
rm -rf .studio

echo
echo "== cold: nothing indexed yet =="
"$BIN" index .

echo
echo "== warm: not one byte changed =="
"$BIN" index .

echo
echo "== one script edited =="
printf '\nfunc probe_added() -> void:\n\tpass\n' >> "scripts/mod1/unit_1.gd"
"$BIN" index .

echo
echo "The warm figure is the cost the studio pays around every command."
echo "Compare it against a command that spawns claude workers for seconds to minutes"
echo "before concluding a filesystem watcher would pay for itself."
