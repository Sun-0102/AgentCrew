// Fail-open streaming adapter for DeepSeek Harness SDK.
// Listens to internal `agent/assistant-stream` events and forwards token deltas
// as JSON-RPC notifications (`ai-gent.stream`). If the internal event contract
// changes in future Harness versions, this plugin degrades silently without
// blocking startup or affecting durable `assistant/message` persistence.

exports.name = "ai-gent-stream";
exports.apply = (ctx) => {
  try {
    if (!ctx || typeof ctx.on !== "function") return;
    ctx.on(
      "agent/assistant-stream",
      (payload) => {
        try {
          const agent = payload && payload.agent;
          const frame = payload && payload.frame;
          const sessionId = agent && agent.session && agent.session.id;
          if (!sessionId || !frame || frame.type !== "chunk") return;
          const chunk = frame.chunk;
          if (!chunk || typeof chunk.text !== "string" || !chunk.text) return;
          let kind = null;
          if (chunk.type === "text-delta") {
            kind = "assistant";
          } else if (chunk.type === "reasoning-delta") {
            kind = "thinking";
          } else {
            return;
          }
          const line = JSON.stringify({
            jsonrpc: "2.0",
            method: "ai-gent.stream",
            params: {
              sessionId: String(sessionId),
              attemptId: String(frame.attemptId || "live"),
              kind,
              text: chunk.text,
            },
          });
          process.stdout.write(line + "\n");
        } catch {
          // Never interfere with the Harness runtime on malformed frames.
        }
      },
      { global: true },
    );
  } catch {
    // Fail open: fall back to durable `assistant/message` events.
  }
};
