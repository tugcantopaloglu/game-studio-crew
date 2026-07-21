const fs = require("fs");

function load(p) {
  return fs
    .readFileSync(p, "utf8")
    .trim()
    .split("\n")
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch {
        return { type: "__unparsed__" };
      }
    });
}

function summarise(label, path) {
  const events = load(path);
  const result = events.find((e) => e.type === "result");

  if (!result) return { label, failed: "no result event" };
  if (result.is_error) return { label, failed: String(result.result).slice(0, 120) };

  const usage = result.usage || {};
  const input = usage.input_tokens || 0;
  const cacheRead = usage.cache_read_input_tokens || 0;
  const cacheWrite = usage.cache_creation_input_tokens || 0;
  const output = usage.output_tokens || 0;

  const calls = events
    .filter((e) => e.type === "assistant")
    .flatMap((e) => e.message?.content || [])
    .filter((c) => c.type === "tool_use")
    .map((c) => c.name);

  return {
    label,
    input,
    cacheRead,
    cacheWrite,
    output,
    billed: input + cacheRead + cacheWrite,
    cost: result.total_cost_usd || 0,
    turns: result.num_turns || 0,
    calls,
    answer: String(result.result || "").replace(/\s+/g, " ").trim().slice(0, 160),
  };
}

const [indexedPath, filesPath] = process.argv.slice(2);
const arms = [summarise("indexed", indexedPath), summarise("files", filesPath)];

for (const arm of arms) {
  if (arm.failed) {
    console.log(`${arm.label.padEnd(9)} FAILED: ${arm.failed}`);
  }
}
if (arms.some((a) => a.failed)) {
  console.log("\nNo comparison: one arm did not complete.");
  process.exit(1);
}

const pad = (n) => String(n).padStart(9);
console.log("arm        input  cache_rd  cache_wr    output     total     turns      cost");
for (const a of arms) {
  console.log(
    a.label.padEnd(9) +
      pad(a.input) +
      pad(a.cacheRead) +
      pad(a.cacheWrite) +
      pad(a.output) +
      pad(a.billed) +
      pad(a.turns) +
      pad("$" + a.cost.toFixed(4))
  );
}

const [indexed, files] = arms;
const ratio = indexed.billed > 0 ? files.billed / indexed.billed : 0;
const costRatio = indexed.cost > 0 ? files.cost / indexed.cost : 0;

console.log();
console.log(`tool calls, indexed: ${indexed.calls.join(", ") || "(none)"}`);
console.log(`tool calls, files:   ${files.calls.join(", ") || "(none)"}`);
console.log();
console.log(`billed input tokens: ${files.billed} reading files vs ${indexed.billed} through the index`);
console.log(`ratio:               ${ratio.toFixed(2)}x tokens, ${costRatio.toFixed(2)}x cost`);
console.log();
console.log("answers (both must be correct for the comparison to mean anything):");
console.log(`  indexed: ${indexed.answer}`);
console.log(`  files:   ${files.answer}`);
