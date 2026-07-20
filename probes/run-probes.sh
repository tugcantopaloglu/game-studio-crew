#!/usr/bin/env bash
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="$HERE/out"
mkdir -p "$OUT"

MODEL="${PROBE_MODEL:-opus}"
COMMON=(--setting-sources "" --output-format stream-json --include-partial-messages --verbose --permission-mode dontAsk)

if [ -n "${CLAUDECODE:-}${CLAUDE_CODE_CHILD_SESSION:-}" ] && [ -z "${PROBE_FORCE:-}" ]; then
  echo "REFUSING TO RUN: this shell is inside a Claude Code session."
  echo
  echo "A nested 'claude' does not inherit credentials and every probe will"
  echo "fail with 'Not logged in', producing misleading verdicts."
  echo
  echo "Open a separate terminal (PowerShell or Windows Terminal), cd to the"
  echo "repo, and run this script there. Using the '!' prefix inside Claude"
  echo "Code is NOT sufficient; it runs in this same session."
  echo
  echo "Override with PROBE_FORCE=1 if you know what you are doing."
  exit 2
fi

echo "probe model: $MODEL"
echo "claude: $(command -v claude || echo MISSING)"
echo

node "$HERE/gen-prefix.js" || exit 1
echo

if command -v cygpath >/dev/null 2>&1; then
  SERVER_JS="$(cygpath -m "$HERE/mcp-probe-server.js")"
else
  SERVER_JS="$HERE/mcp-probe-server.js"
fi

cat > "$OUT/mcp.json" <<JSON
{
  "mcpServers": {
    "probe": {
      "command": "node",
      "args": ["$SERVER_JS"]
    }
  }
}
JSON

echo "mcp server path: $SERVER_JS"
echo

echo "=== PROBE A: stream shape and interim usage ==="
claude "${COMMON[@]}" --model "$MODEL" \
  --system-prompt-file "$HERE/prefix.txt" --tools "" \
  -p "Count from 1 to 20, one number per line." \
  < /dev/null > "$OUT/a.ndjson" 2>&1
echo "exit=$?"
node "$HERE/analyze.js" shape "$OUT/a.ndjson"
echo

echo "=== PROBE B: MCP attachment ==="
claude "${COMMON[@]}" --model "$MODEL" \
  --system-prompt-file "$HERE/prefix.txt" \
  --mcp-config "$OUT/mcp.json" --strict-mcp-config \
  --tools "Read" --allowedTools "mcp__probe__ping,Read" \
  -p "Call the ping tool exactly once, then reply with whatever text it returned." \
  < /dev/null > "$OUT/b.ndjson" 2>&1
echo "exit=$?"
node "$HERE/analyze.js" mcp "$OUT/b.ndjson"
echo

echo "=== PROBE C: prompt cache across separate subprocesses ==="
for n in 1 2; do
  claude "${COMMON[@]}" --model "$MODEL" \
    --system-prompt-file "$HERE/prefix.txt" \
    --tools "Read,Grep,Glob" \
    -p "Reply with exactly the word: pong" \
    < /dev/null > "$OUT/c$n.ndjson" 2>&1
  echo "run $n exit=$?"
done
node "$HERE/analyze.js" cache "$OUT/c1.ndjson" "$OUT/c2.ndjson"
echo

echo "raw output kept in $OUT"
