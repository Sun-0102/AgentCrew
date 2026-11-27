import { after, test } from "node:test";
import assert from "node:assert/strict";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const server = await createServer({
  configFile: false,
  server: { middlewareMode: true, hmr: false, watch: { ignored: ["**/src-tauri/**"] } },
  optimizeDeps: { noDiscovery: true },
  appType: "custom",
});
after(() => server.close());

const { default: UsageDashboard, PROVIDER_SERIES } = await server.ssrLoadModule(
  "/src/features/usage/UsageDashboard.tsx",
);

test("uses four distinct provider CSS color tokens and distinct marker shapes", () => {
  const colors = new Set(PROVIDER_SERIES.map((s) => s.colorVar));
  const markers = new Set(PROVIDER_SERIES.map((s) => s.marker));
  assert.equal(colors.size, 4);
  assert.equal(markers.size, 4);
});

test("renders saved cache badge, low-cost DeepSeek legend stats, and suppresses idle zero markers", () => {
  const html = renderToStaticMarkup(
    React.createElement(UsageDashboard, {
      days: 7,
      loading: false,
      embedded: true,
      onChangeDays: () => {},
      onRefresh: () => {},
      data: {
        updatedAt: 1795768000,
        rangeDays: 7,
        granularity: "day",
        codexLimits: { available: false, windows: [], resetCreditsCount: 0 },
        summary: {
          totalCostUsd: 9.25,
          savedCostUsd: 2.4,
          totalInputTokens: 1_500_000,
          totalOutputTokens: 120_000,
          totalCachedTokens: 1_100_000,
          cacheHitRate: 73.3,
          totalTurns: 18,
          groupTurns: 12,
          privateTurns: 6,
          completedTurns: 18,
          failedTurns: 0,
          activeDays: 3,
        },
        daily: [
          {
            date: "11-26",
            codexCost: 3.2,
            deepseekCost: 0,
            geminiCost: 5.2,
            claudeCost: 0,
            codexTurns: 4,
            deepseekTurns: 0,
            geminiTurns: 6,
            claudeTurns: 0,
            codexTokens: 400_000,
            deepseekTokens: 0,
            geminiTokens: 800_000,
            claudeTokens: 0,
          },
          {
            date: "11-27",
            codexCost: 0.82,
            deepseekCost: 0.03,
            geminiCost: 0,
            claudeCost: 0,
            codexTurns: 3,
            deepseekTurns: 5,
            geminiTurns: 0,
            claudeTurns: 0,
            codexTokens: 180_000,
            deepseekTokens: 240_000,
            geminiTokens: 0,
            claudeTokens: 0,
          },
        ],
        models: [
          {
            providerKind: "gemini",
            agentId: "gemini",
            model: "gemini-3-pro",
            turns: 6,
            inputTokens: 750_000,
            outputTokens: 50_000,
            cachedTokens: 600_000,
            cacheHitRate: 80,
            costUsd: 5.2,
          },
          {
            providerKind: "deepseek",
            agentId: "deepseek",
            model: "deepseek-chat",
            turns: 5,
            inputTokens: 210_000,
            outputTokens: 30_000,
            cachedTokens: 150_000,
            cacheHitRate: 71.4,
            costUsd: 0.03,
          },
        ],
        agents: [],
      },
    }),
  );

  assert.match(html, /约省 \$2\.40/);
  assert.match(html, /\$0\.030 \/ 5轮/);
  assert.match(html, /微额放大/);
  assert.doesNotMatch(html, /·/);
});
