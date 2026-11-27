import { useState, type ComponentProps } from "react";
import ChatStage from "./ChatStage";
import { useAgents, type Probe } from "./api";
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
};

interface Preferences {
  models: Record<string, string>;
  efforts: Record<string, string>;
  tiers: Record<string, string>;
}

export default function ConnectedChat(p: Props) {
  const registry = useAgents();
  const [error, setError] = useState("");
  const preferencesKey = p.scope.projectId && !p.group
    ? `ai-gent.instance-models.v1.${p.scopeKey}`
    : "ai-gent.instance-models.v1";
  const [preferences, setPreferences] = useState<Preferences>(() => {
    try {
      const oldEfforts = JSON.parse(localStorage.getItem("ai-gent.reasoning.v1") ?? "{}");
      const oldTiers = Object.fromEntries(
        Object.entries(
          JSON.parse(localStorage.getItem("ai-gent.speed.v1") ?? "{}") as Record<string, string>,
        ).map(([model, tier]) => [`codex:${model}`, tier]),
      );
      return {
        models: {},
        efforts: oldEfforts,
        tiers: oldTiers,
        ...JSON.parse(localStorage.getItem("ai-gent.instance-models.v1") ?? "{}"),
        ...(preferencesKey !== "ai-gent.instance-models.v1"
          ? JSON.parse(localStorage.getItem(preferencesKey) ?? "{}")
          : {}),
      };
    } catch {
      return { models: {}, efforts: {}, tiers: {} };
    }
  });

  const agent = p.agent ?? "codex";
  const ids = p.group?.members ?? [agent];

  function save(next: Preferences) {
    setPreferences(next);
    try {
      localStorage.setItem(preferencesKey, JSON.stringify(next));
    } catch {
      setError("模型设置未能保存，重启后需要重新选择。");
    }
  }

  const models: Record<string, string> = {};
  const efforts: Record<string, string> = {};
  const tiers: Record<string, string> = {};

  for (const id of ids) {
    const probe = p.probes[id];
    const saved =
      preferences.models[id] ??
      p.snapshot.sessions?.[id]?.model ??
      (id === agent ? p.snapshot.session?.model : undefined);
    models[id] =
      probe?.models.find(m => m.model === saved)?.model ??
      probe?.models.find(m => m.isDefault)?.model ??
      probe?.models[0]?.model ??
      "";
    const model = probe?.models.find(m => m.model === models[id]);
    efforts[id] = selectedEffort(model, preferences.efforts[`${id}:${models[id]}`]);
    tiers[id] = selectedSpeed(model, preferences.tiers[`${id}:${models[id]}`]);
  }

  const probe = p.probes[agent] ?? null;
  const current = models[agent] ?? "";
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
        onModel={model => save({ ...preferences, models: { ...preferences.models, [agent]: model } })}
        collaborationModels={dispatchModels}
        collaborationEfforts={efforts}
        collaborationTiers={tiers}
        reasoningEffort={efforts[agent]}
        onReasoningEffort={value =>
          save({ ...preferences, efforts: { ...preferences.efforts, [`${agent}:${current}`]: value } })
        }
        codexServiceTier={tiers[agent]}
        onServiceTier={value =>
          save({ ...preferences, tiers: { ...preferences.tiers, [`${agent}:${current}`]: value } })
        }
      />
    </>
  );
}
