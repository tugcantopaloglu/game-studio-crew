const TOOL = {
  name: "ping",
  description: "Probe tool. Returns the literal string PROBE_MCP_OK.",
  inputSchema: { type: "object", properties: {}, additionalProperties: false },
};

let buf = "";

process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buf += chunk;
  let nl;
  while ((nl = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, nl).trim();
    buf = buf.slice(nl + 1);
    if (line) handle(line);
  }
});

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

function handle(line) {
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method } = req;

  if (method === "initialize") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: { tools: {} },
        serverInfo: { name: "probe", version: "1.0.0" },
      },
    });
    return;
  }

  if (method === "notifications/initialized") return;

  if (method === "tools/list") {
    send({ jsonrpc: "2.0", id, result: { tools: [TOOL] } });
    return;
  }

  if (method === "tools/call") {
    process.stderr.write("PROBE_SERVER_TOOL_INVOKED\n");
    send({
      jsonrpc: "2.0",
      id,
      result: { content: [{ type: "text", text: "PROBE_MCP_OK" }] },
    });
    return;
  }

  if (id !== undefined) {
    send({
      jsonrpc: "2.0",
      id,
      error: { code: -32601, message: `method not found: ${method}` },
    });
  }
}
