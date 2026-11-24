import { after, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const server = await createServer({
  configFile: false,
  server: { middlewareMode: true, hmr: false, watch: { ignored: ["**/src-tauri/**"] } },
  optimizeDeps: { noDiscovery: true },
  appType: "custom",
});
after(() => server.close());

const { default: ContextMeter, formatCompactTokens, formatPercent } = await server.ssrLoadModule(
  "/src/features/chats/ContextMeter.tsx",
);

test("formats compact token counts and percentages cleanly", () => {
  assert.equal(formatCompactTokens(0), "0");
  assert.equal(formatCompactTokens(48200), "48.2k");
  assert.equal(formatCompactTokens(272000), "272k");
  assert.equal(formatCompactTokens(1048576), "1M");
  assert.equal(formatPercent(0.4), "<1%");
  assert.equal(formatPercent(17.7), "18%");
});

test("renders selected agent meter inline and lists all group members in popover", () => {
  const html = renderToStaticMarkup(
    createElement(ContextMeter, {
      agent: "codex",
      members: ["codex", "gemini"],
      isGroup: true,
      model: "gpt-5.4",
      contextUsage: {
        codex: {
          agentId: "codex",
          providerKind: "codex",
          model: "gpt-5.4",
          usedTokens: 48200,
          contextWindow: 272000,
          percent: 17.7,
          inputTokens: 46100,
          outputTokens: 2100,
          cachedTokens: 32000,
          source: "live",
        },
        gemini: {
          agentId: "gemini",
          providerKind: "gemini",
          model: "gemini-3-pro",
          usedTokens: 31000,
          contextWindow: 1048576,
          percent: 3.0,
          inputTokens: 29000,
          outputTokens: 2000,
          cachedTokens: 0,
          source: "recorded",
        },
      },
    }),
  );
  assert.match(html, /18%/);
  assert.match(html, /48\.2k \/ 272k/);
  assert.match(html, /群成员上下文/);
  assert.match(html, /31k \/ 1M/);
});
