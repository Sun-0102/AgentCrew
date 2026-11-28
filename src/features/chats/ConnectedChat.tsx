import { useState, type ComponentProps } from "react";
import ChatStage from "./ChatStage";
import { agentName, useAgents, type Probe } from "./api";
import {
  readSessionOverrides,
  resolveModelDefault,
  writeSessionOverrides,
  type ModelDefaultScope,
  type ModelDefaultsConfig,
  type SessionOverrides,
} from "./modelDefaults";
import { selectedEffort } from "./reasoning";
import { selectedSpeed } from "./speed";

type Props = Pick<
  ComponentProps<typeof ChatStage>,
  | "scope"
  | "scopeKey"
  | "title"
  | "group"
  | "project"
  | "agent"
  | "snapshot"
  | "onAgent"
  | "onInvite"
  | "onSettings"
  | "onRefresh"
  | "notify"
  | "session"
  | "onGroups"
  | "onRenameSession"
  | "onArchiveSession"
  | "sessionSaving"
  | "onDeleteSession"
  | "onGroupUpdated"
  | "sidebarControls"
  | "onOpenGit"
  | "isDraft"
  | "onCreateDraftSession"
  | "onCommitDraftSession"
> & {
  probes: Record<string, Probe>;
  connectingByAgent: Record<string, boolean>;
  onConnectAgent: (id: string) => void;
  modelDefaults: ModelDefaultsConfig;
  onSaveModelDefault: (params: {
    scope: ModelDefaultScope;
    agentId: string;
    model: string;
    reasoningEffort?: string | null;
    serviceTier?: string | null;
  }) => Promise<void>;
};

export default function ConnectedChat(p: Props) {
  const registry = useAgents();
  const [error, setError] = useState("");
  const [overrides, setOverrides] = useState<SessionOverrides>(() =>
    readSessionOverrides(p.scopeKey),
  );

  const agent = p.agent ?? "codex";
  const ids = p.group?.members ?? [agent];
  const sceneScope: ModelDefaultScope = p.group ? "group" : "private";
  const saveTargetScope: ModelDefaultScope =
    p.group && !p.modelDefaults.group_follow_private ? "group" : "private";
  const defaultScopeLabel =
    saveTargetScope === "group" ? "群聊默认" : p.group ? "通用默认" : "私聊默认";

  function save(next: SessionOverrides) {
    setOverrides(next);
    try {
      writeSessionOverrides(p.scopeKey, next);
    } catch {
      setError("模型设置未能保存，重启后需要重新选择。");
    }
  }

  const models: Record<string, string> = {};
  const efforts: Record<string, string> = {};
  const tiers: Record<string, string> = {};

  for (const id of ids) {
    const probe = p.probes[id];
    const sceneDefault = resolveModelDefault(p.modelDefaults, sceneScope, id);
    const snapshotModel =
      p.snapshot.sessions?.[id]?.model ??
      (id === agent ? p.snapshot.session?.model : undefined);
    const candidates = [
      overrides.models[id],
      snapshotModel,
      sceneDefault?.model,
    ].filter((value): value is string => !!value && value.trim().length > 0);

    let resolvedModel = "";
    if (probe && probe.models.length > 0) {
      for (const candidate of candidates) {
        const hit = probe.models.find(m => m.model === candidate);
        if (hit) {
          resolvedModel = hit.model;
          break;
        }
      }
      if (!resolvedModel) {
        resolvedModel =
          probe.models.find(m => m.isDefault)?.model ??
          probe.models[0]?.model ??
          "";
      }
    } else {
      resolvedModel = candidates[0] ?? "";
    }

    models[id] = resolvedModel;
    const modelObj = probe?.models.find(m => m.model === resolvedModel);
    const savedEffort =
      overrides.efforts[`${id}:${resolvedModel}`] ??
      sceneDefault?.reasoning_effort ??
      undefined;
    const savedTier =
      overrides.tiers[`${id}:${resolvedModel}`] ??
      sceneDefault?.service_tier ??
      undefined;
    efforts[id] = selectedEffort(modelObj, savedEffort);
    tiers[id] = selectedSpeed(modelObj, savedTier);
  }

  const probe = p.probes[agent] ?? null;
  const current = models[agent] ?? "";
  const currentModelObj = probe?.models.find(m => m.model === current);
  const currentDefault = resolveModelDefault(p.modelDefaults, sceneScope, agent);
  const defaultResolvedModel =
    (currentDefault?.model &&
    probe?.models.some(m => m.model === currentDefault.model)
      ? currentDefault.model
      : probe?.models.find(m => m.isDefault)?.model ??
        probe?.models[0]?.model ??
        "");
  const defaultModelObj = probe?.models.find(
    m => m.model === defaultResolvedModel,
  );
  const defaultEffort = selectedEffort(
    defaultModelObj,
    currentDefault?.reasoning_effort ?? undefined,
  );
  const defaultTier = selectedSpeed(
    defaultModelObj,
    currentDefault?.service_tier ?? undefined,
  );
  const canSaveAsDefault =
    !!probe &&
    !!current &&
    (current !== defaultResolvedModel ||
      (efforts[agent] ?? "") !== defaultEffort ||
      (tiers[agent] ?? "default") !== defaultTier);

  const dispatchModels = Object.fromEntries(
    Object.entries(models).map(([id, model]) => [
      id,
      registry.find(a => a.id === id)?.available &&
      (!p.probes[id]?.requires_auth || !!p.probes[id]?.auth) &&
      !p.connectingByAgent[id]
        ? model
        : "",
    ]),
  );

  async function saveCurrentAsDefault() {
    if (!current) return;
    try {
      await p.onSaveModelDefault({
        scope: saveTargetScope,
        agentId: agent,
        model: current,
        reasoningEffort: efforts[agent] || null,
        serviceTier: tiers[agent] && tiers[agent] !== "default" ? tiers[agent] : null,
      });
      const label = currentModelObj?.displayName ?? current;
      p.notify(`已将 ${agentName(agent)} 的${defaultScopeLabel}设为 ${label}`);
    } catch (cause) {
      setError(`保存默认配置失败：${String(cause)}`);
    }
  }

  return (
    <>
      {error && (
        <div className="global-error" role="alert">
          <span>{error}</span>
          <button className="outline-button" onClick={() => p.onConnectAgent(agent)}>
            重试检查
          </button>
        </div>
      )}
      <ChatStage
        {...p}
        probe={probe}
        model={current}
        connecting={!!p.connectingByAgent[agent]}
        onConnect={() => p.onConnectAgent(agent)}
        onModel={model =>
          save({ ...overrides, models: { ...overrides.models, [agent]: model } })
        }
        collaborationModels={dispatchModels}
        collaborationEfforts={efforts}
        collaborationTiers={tiers}
        reasoningEffort={efforts[agent]}
        onReasoningEffort={value =>
          save({
            ...overrides,
            efforts: { ...overrides.efforts, [`${agent}:${current}`]: value },
          })
        }
        codexServiceTier={tiers[agent]}
        onServiceTier={value =>
          save({
            ...overrides,
            tiers: { ...overrides.tiers, [`${agent}:${current}`]: value },
          })
        }
        canSaveAsDefault={canSaveAsDefault}
        defaultScopeLabel={defaultScopeLabel}
        onSaveAsDefault={probe ? () => void saveCurrentAsDefault() : undefined}
      />
    </>
  );
}
