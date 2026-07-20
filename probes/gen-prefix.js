const fs = require("fs");
const path = require("path");

const TARGET_TOKENS = 5000;
const CHARS_PER_TOKEN = 3.6;
const TARGET_CHARS = Math.ceil(TARGET_TOKENS * CHARS_PER_TOKEN);

const HEAD = `You are a probe worker for the game-studio-crew orchestrator.

This charter exists to test prompt caching behaviour. It is deliberately
padded past the Opus minimum cacheable prefix of 4096 tokens with stable,
meaningful text. The bytes of this file must never vary between runs:
no timestamps, no interpolation, no run identifiers.

Studio conventions follow.

`;

const CONVENTIONS = [
  "Capsules are the only inter-agent channel. A worker emits exactly one capsule and never addresses another worker directly.",
  "Artifacts are passed by reference. A capsule carries paths and symbol names, never file bodies.",
  "Dead ends are recorded in do_not_revisit so the next worker does not re-derive the same failure.",
  "Escalation goes to the role's declared parent. A worker never escalates laterally.",
  "Verification belongs to the daemon. A worker never parses raw engine logs.",
  "The frozen prefix carries no per-run identifiers. Everything volatile belongs to the task brief.",
  "Budget is enforced by the daemon. A worker that receives a budget warning summarizes and returns.",
  "Decisions that bind other roles are promoted to ADRs and cited by identifier thereafter.",
  "Symbol slices are pulled on demand. A worker requests a full body only when the signature is insufficient.",
  "Engine specialization is injected as a prompt layer. A role charter never names an engine.",
];

let body = HEAD;
let i = 0;
while (body.length < TARGET_CHARS) {
  const c = CONVENTIONS[i % CONVENTIONS.length];
  body += `${String(i + 1).padStart(4, "0")}. ${c}\n`;
  i++;
}

body += "\nWhen asked to reply, reply with exactly the single word: pong\n";

const out = path.join(__dirname, "prefix.txt");
fs.writeFileSync(out, body.replace(/\r\n/g, "\n"), "utf8");

console.log(`wrote ${out}`);
console.log(`chars: ${body.length}  approx tokens: ${Math.round(body.length / CHARS_PER_TOKEN)}`);
