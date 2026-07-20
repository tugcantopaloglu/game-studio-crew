#!/usr/bin/env bash
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="$HERE/out"
mkdir -p "$OUT"

MODEL="${PROBE_MODEL:-opus}"
COMMON=(--bare --output-format stream-json --include-partial-messages --verbose --permission-mode dontAsk)

echo "probe model: $MODEL"
echo "claude: $(command -v claude || echo MISSING)"
echo

node "$HERE/gen-prefix.js" || exit 1
echo

cat > "$OUT/mcp.json" <<JSON
{
  "mcpServers": {
    "probe": {
      "command": "node",
      "args": ["$HERE/mcp-probe-server.js"]
    }
  }
}
JSON

echo "=== PROBE A: stream shape and interim usage ==="
claude "${COMMON[@]}" --model "$MODEL" \
  -p "Reply with exactly the word: pong" \
  < /dev/null > "$OUT/a.ndjson" 2>&1
echo "exit=$?"
node "$HERE/analyze.js" shape "$OUT/a.ndjson"
echo

echo "=== PROBE B: --mcp-config under --bare ==="
claude "${COMMON[@]}" --model "$MODEL" \
  --mcp-config "$OUT/mcp.json" \
  --allowedTools "mcp__probe__ping" \
  -p "Call the ping tool exactly once, then reply with whatever text it returned." \
  < /dev/null > "$OUT/b.ndjson" 2>&1
echo "exit=$?"
node "$HERE/analyze.js" mcp "$OUT/b.ndjson"
echo

echo "=== PROBE C: prompt cache across separate subprocesses ==="
for n in 1 2; do
  claude "${COMMON[@]}" --model "$MODEL" \
    --system-prompt-file "$HERE/prefix.txt" \
    -p "Reply with exactly the word: pong" \
    < /dev/null > "$OUT/c$n.ndjson" 2>&1
  echo "run $n exit=$?"
done
node "$HERE/analyze.js" cache "$OUT/c1.ndjson" "$OUT/c2.ndjson"
echo

echo "raw output kept in $OUT"
