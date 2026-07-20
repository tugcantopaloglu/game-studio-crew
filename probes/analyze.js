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
        return { type: "__unparsed__", raw: l };
      }
    });
}

function result(evts) {
  return evts.find((e) => e.type === "result");
}

function fail(evts) {
  const r = result(evts);
  if (!r) return "no result event";
  if (r.is_error) return `is_error: ${r.result}`;
  return null;
}

const [mode, ...files] = process.argv.slice(2);

if (mode === "shape") {
  const e = load(files[0]);
  const bad = fail(e);
  if (bad) {
    console.log(`  FAILED: ${bad}`);
    process.exit(0);
  }
  const kinds = new Map();
  for (const o of e) {
    const k = o.type + (o.event?.type ? "/" + o.event.type : "") + (o.subtype ? "/" + o.subtype : "");
    kinds.set(k, (kinds.get(k) || 0) + 1);
  }
  console.log("  event types seen:");
  for (const [k, n] of kinds) console.log(`    ${k} x${n}`);

  const interim = e.filter(
    (o) => o.type !== "result" && JSON.stringify(o).includes("output_tokens")
  );
  console.log(`  events carrying usage before result: ${interim.length}`);
  if (interim.length) {
    const s = interim[0];
    const u = s.usage || s.event?.usage || s.event?.message?.usage || s.message?.usage;
    console.log(`    sample: ${(s.type || "") + (s.event?.type ? "/" + s.event.type : "")} -> ${JSON.stringify(u)}`);
  }
  console.log(
    `  VERDICT interim-usage-deltas: ${interim.length > 1 ? "AVAILABLE" : "NOT AVAILABLE (use EMA fallback)"}`
  );
}

if (mode === "mcp") {
  const e = load(files[0]);
  const bad = fail(e);
  const init = e.find((o) => o.type === "system" && o.subtype === "init");
  const servers = init?.mcp_servers ?? null;
  console.log(`  init.mcp_servers: ${JSON.stringify(servers)}`);
  const registered = Array.isArray(servers) && servers.some((s) => s.name === "probe");
  const sawTool = JSON.stringify(e).includes("mcp__probe__ping");
  const sawResult = JSON.stringify(e).includes("PROBE_MCP_OK");
  console.log(`  server registered under --bare: ${registered}`);
  console.log(`  tool advertised in session: ${sawTool}`);
  console.log(`  tool actually returned value: ${sawResult}`);

  if (bad) {
    console.log(`  RUN FAILED: ${bad}`);
    console.log(
      registered
        ? "  PARTIAL: --mcp-config survived --bare (server registered) but the run died before connect. Rerun authenticated to confirm."
        : "  INCONCLUSIVE: run failed before MCP registration could be observed."
    );
    return;
  }

  console.log(
    `  VERDICT mcp-under-bare: ${sawResult ? "ATTACHES" : sawTool ? "ADVERTISED BUT NOT CALLED" : "DOES NOT ATTACH (use outbox fallback)"}`
  );
}

if (mode === "cache") {
  const runs = files.map(load);
  for (const [i, e] of runs.entries()) {
    const bad = fail(e);
    if (bad) {
      console.log(`  run ${i + 1} FAILED: ${bad}`);
      return;
    }
  }
  const u = runs.map((e) => result(e).usage);
  for (const [i, x] of u.entries()) {
    console.log(
      `  run ${i + 1}: input=${x.input_tokens} cache_write=${x.cache_creation_input_tokens} cache_read=${x.cache_read_input_tokens}`
    );
  }
  const wrote = u[0].cache_creation_input_tokens > 0;
  const read = u[1].cache_read_input_tokens > 0;
  console.log(`  VERDICT cache-across-subprocesses: ${wrote && read ? "CONFIRMED" : "NOT CONFIRMED"}`);
  if (wrote && !read) console.log("    wrote cache but second run did not read it");
  if (!wrote) console.log("    first run created no cache entry; prefix may be under the minimum");
}
