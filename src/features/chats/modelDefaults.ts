import { invoke } from "@tauri-apps/api/core";
import { desktop } from "./api";

export type ModelDefaultScope = "private" | "group";

export interface ModelDefaultEntry {
  scope: ModelDefaultScope;
  agent_id: string;
  model: string;
  reasoning_effort: string | null;
  service_tier: string | null;
}

export interface ModelDefaultsConfig {
  group_follow_private: boolean;
  entries: ModelDefaultEntry[];
}

export interface SessionOverrides {
  models: Record<string, string>;
  efforts: Record<string, string>;
  tiers: Record<string, string>;
}

const PREVIEW_DEFAULTS_KEY = "ai-gent.model-defaults.preview.v1";
const OVERRIDE_PREFIX = "ai-gent.session-overrides.v1.";

export const emptyModelDefaults: ModelDefaultsConfig = {
  group_follow_private: true,
  entries: [],
};

export function resolveModelDefault(
  config: ModelDefaultsConfig,
  scope: ModelDefaultScope,
  agentId: string,
): ModelDefaultEntry | undefined {
  const effectiveScope: ModelDefaultScope =
    scope === "group" && config.group_follow_private ? "private" : scope;
  return config.entries.find(
    entry => entry.scope === effectiveScope && entry.agent_id === agentId,
  );
}

export function findDirectModelDefault(
  config: ModelDefaultsConfig,
  scope: ModelDefaultScope,
  agentId: string,
): ModelDefaultEntry | undefined {
  return config.entries.find(
    entry => entry.scope === scope && entry.agent_id === agentId,
  );
}

function readPreviewDefaults(): ModelDefaultsConfig {
  try {
    const raw = localStorage.getItem(PREVIEW_DEFAULTS_KEY);
    if (!raw) return emptyModelDefaults;
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      return {
        group_follow_private: parsed.group_follow_private !== false,
        entries: Array.isArray(parsed.entries) ? parsed.entries : [],
      };
    }
  } catch {
    // ignore
  }
  return emptyModelDefaults;
}

function writePreviewDefaults(config: ModelDefaultsConfig) {
  try {
    localStorage.setItem(PREVIEW_DEFAULTS_KEY, JSON.stringify(config));
  } catch {
    // ignore
  }
}

export async function getModelDefaults(): Promise<ModelDefaultsConfig> {
  if (!desktop) return readPreviewDefaults();
  return invoke<ModelDefaultsConfig>("model_defaults_get");
}

export async function saveModelDefault(params: {
  scope: ModelDefaultScope;
  agentId: string;
  model: string;
  reasoningEffort?: string | null;
  serviceTier?: string | null;
}): Promise<ModelDefaultsConfig> {
  const cleanEffort = params.reasoningEffort?.trim() || null;
  const cleanTier =
    params.serviceTier?.trim() && params.serviceTier.trim() !== "default"
      ? params.serviceTier.trim()
      : null;
  if (!desktop) {
    const current = readPreviewDefaults();
    const filtered = current.entries.filter(
      e => !(e.scope === params.scope && e.agent_id === params.agentId),
    );
    const nextEntries =
      !params.model.trim() && !cleanEffort && !cleanTier
        ? filtered
        : [
            ...filtered,
            {
              scope: params.scope,
              agent_id: params.agentId,
              model: params.model.trim(),
              reasoning_effort: cleanEffort,
              service_tier: cleanTier,
            },
          ];
    const next: ModelDefaultsConfig = {
      ...current,
      entries: nextEntries,
    };
    writePreviewDefaults(next);
    return next;
  }
  return invoke<ModelDefaultsConfig>("model_defaults_save", {
    scope: params.scope,
    agentId: params.agentId,
    model: params.model.trim(),
    reasoningEffort: cleanEffort,
    serviceTier: cleanTier,
  });
}

export async function setGroupFollowPrivate(
  follow: boolean,
): Promise<ModelDefaultsConfig> {
  if (!desktop) {
    const current = readPreviewDefaults();
    const next: ModelDefaultsConfig = {
      ...current,
      group_follow_private: follow,
    };
    writePreviewDefaults(next);
    return next;
  }
  return invoke<ModelDefaultsConfig>("model_defaults_set_group_follow", {
    follow,
  });
}

export function sessionOverrideKey(scopeKey: string): string {
  return `${OVERRIDE_PREFIX}${scopeKey}`;
}

export function readSessionOverrides(scopeKey: string): SessionOverrides {
  try {
    const raw = localStorage.getItem(sessionOverrideKey(scopeKey));
    if (!raw) return { models: {}, efforts: {}, tiers: {} };
    const parsed = JSON.parse(raw);
    return {
      models:
        parsed?.models && typeof parsed.models === "object"
          ? parsed.models
          : {},
      efforts:
        parsed?.efforts && typeof parsed.efforts === "object"
          ? parsed.efforts
          : {},
      tiers:
        parsed?.tiers && typeof parsed.tiers === "object" ? parsed.tiers : {},
    };
  } catch {
    return { models: {}, efforts: {}, tiers: {} };
  }
}

export function writeSessionOverrides(
  scopeKey: string,
  next: SessionOverrides,
): void {
  localStorage.setItem(sessionOverrideKey(scopeKey), JSON.stringify(next));
}

export function clearSessionOverrides(scopeKey: string): void {
  try {
    localStorage.removeItem(sessionOverrideKey(scopeKey));
  } catch {
    // ignore
  }
}

export function transferDraftOverrides(
  fromScopeKey: string,
  toScopeKey: string,
): void {
  if (fromScopeKey === toScopeKey) return;
  try {
    const raw = localStorage.getItem(sessionOverrideKey(fromScopeKey));
    if (raw) {
      localStorage.setItem(sessionOverrideKey(toScopeKey), raw);
      localStorage.removeItem(sessionOverrideKey(fromScopeKey));
    }
  } catch {
    // ignore
  }
}
