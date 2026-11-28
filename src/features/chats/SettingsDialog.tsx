import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Dialog from "../../components/Dialog";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import type { AppInfo } from "../projects/api";
import UsageDashboard from "../usage/UsageDashboard";
import type { UsageDashboardData } from "../usage/api";
import { desktop, getAutoCompactContext, refreshAgents, setAutoCompactContext, type Agent, type DiscoveryItem, type Model, type Probe, type ProviderCapabilities } from "./api";
import {
  findDirectModelDefault,
  type ModelDefaultScope,
  type ModelDefaultsConfig,
} from "./modelDefaults";
import { effortLabels } from "./reasoning";
import { speedChoices } from "./speed";
import WikiSettingsPanel from "../wiki/WikiSettingsPanel";

export type SettingsTab = "agents" | "defaults" | "usage" | "wiki";

interface Props {
  initialTab?: SettingsTab;
  agents: Agent[];
  discoveryResults: DiscoveryItem[];
  discovering: boolean;
  probes: Record<string, Probe>;
  probing: Record<string, boolean>;
  paused: boolean;
  info: AppInfo | null;
  usageData: UsageDashboardData;
  usageDays: number;
  usageLoading: boolean;
  modelDefaults: ModelDefaultsConfig;
  onSaveModelDefault: (params: {
    scope: ModelDefaultScope;
    agentId: string;
    model: string;
    reasoningEffort?: string | null;
    serviceTier?: string | null;
  }) => Promise<void>;
  onToggleGroupFollowPrivate: (follow: boolean) => Promise<void>;
  onChangeUsageDays: (days: number) => void;
  onRefreshUsage: () => void;
  onDiscover: () => void;
  onProbeAgent: (agent: Agent) => void;
  onOpenPairing: () => void;
  onToggleDispatch: () => void;
  onError: (message: string) => void;
  onClose: () => void;
}

function formatVersion(version?: string | null): string | null {
  if (!version) return null;
  const trimmed = version.trim();
  if (!trimmed) return null;
  return /^[0-9]/.test(trimmed) ? `v${trimmed}` : trimmed;
}

function capabilityTags(caps?: ProviderCapabilities | null): string[] {
  if (!caps) return [];
  const tags: string[] = [];
  if (caps.tools) tags.push("工具");
  if (caps.approval) tags.push("审批");
  if (caps.resume) tags.push("恢复");
  if (caps.cancel) tags.push("取消");
  if (caps.acp) tags.push("ACP");
  return tags;
}

function memberStatus(agent: Agent, discovery?: DiscoveryItem, isProbing?: boolean): { label: string; tone: "ready" | "warn" | "busy" | "muted" } {
  if (isProbing || agent.registration_state === "validating" || agent.availability === "checking") {
    return { label: "检测中…", tone: "busy" };
  }
  if (!agent.enabled) return { label: "已停用", tone: "muted" };
  if (agent.registration_state !== "active") return { label: "接入失败", tone: "warn" };
  if (discovery?.status === "needs_auth" || (agent.availability === "degraded" && agent.error)) {
    return { label: "需原厂登录", tone: "warn" };
  }
  if (agent.availability === "ready") return { label: "已就绪", tone: "ready" };
  return { label: "未连接", tone: "muted" };
}

export default function SettingsDialog({
  initialTab = "agents",
  agents,
  discoveryResults,
  discovering,
  probes,
  probing,
  paused,
  info,
  usageData,
  usageDays,
  usageLoading,
  modelDefaults,
  onSaveModelDefault,
  onToggleGroupFollowPrivate,
  onChangeUsageDays,
  onRefreshUsage,
  onDiscover,
  onProbeAgent,
  onOpenPairing,
  onToggleDispatch,
  onError,
  onClose,
}: Props) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const [defaultScope, setDefaultScope] = useState<ModelDefaultScope>("private");
  const [savingDefaultKey, setSavingDefaultKey] = useState<string | null>(null);
  const [autoCompact, setAutoCompact] = useState(true);
  const [savingAutoCompact, setSavingAutoCompact] = useState(false);

  useEffect(() => {
    if (!desktop) return;
    void getAutoCompactContext()
      .then(v => setAutoCompact(v))
      .catch(() => {});
  }, []);

  async function toggleAutoCompact(next: boolean) {
    setSavingAutoCompact(true);
    setAutoCompact(next);
    try {
      await setAutoCompactContext(next);
    } catch (err) {
      setAutoCompact(!next);
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingAutoCompact(false);
    }
  }

  useEffect(() => {
    setTab(initialTab);
  }, [initialTab]);

  useEffect(() => {
    if (tab === "usage") {
      onRefreshUsage();
    }
  }, [tab]);

  const activeMembers = agents.filter(a => a.registration_state !== "revoked");
  const discoveryByProvider = new Map(discoveryResults.map(item => [item.provider_kind, item]));
  const registeredProviders = new Set(activeMembers.map(a => a.provider_kind));
  const unregisteredDiscoveries = discoveryResults.filter(item => !registeredProviders.has(item.provider_kind));

  const hasUsageWarning =
    usageData.agents.some(a => a.quotaExhaustedRecent) ||
    usageData.codexLimits.windows.some(w => w.remainingPercent <= 20);

  const runtimePaths: { id: string; label: string; version: string | null; executable: string }[] = [];
  for (const member of activeMembers) {
    const liveProbe = probes[member.id];
    const disc = discoveryByProvider.get(member.provider_kind);
    const executable = liveProbe?.executable ?? disc?.executable ?? member.executable_path ?? null;
    if (executable) {
      runtimePaths.push({
        id: member.id,
        label: member.name,
        version: formatVersion(liveProbe?.version ?? disc?.version ?? member.runtime_version),
        executable,
      });
    }
  }

  async function toggleEnabled(agent: Agent) {
    try {
      await invoke("agent_update", { agent: agent.id, enabled: !agent.enabled, revoke: false });
      await refreshAgents();
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function revokeAgent(agent: Agent) {
    try {
      await invoke("agent_update", { agent: agent.id, enabled: false, revoke: true });
      await refreshAgents();
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function saveAgentDefault(
    scope: ModelDefaultScope,
    agentId: string,
    model: string,
    reasoningEffort: string | null,
    serviceTier: string | null,
  ) {
    const key = `${scope}:${agentId}`;
    setSavingDefaultKey(key);
    try {
      await onSaveModelDefault({
        scope,
        agentId,
        model,
        reasoningEffort,
        serviceTier,
      });
    } catch (cause) {
      onError(`保存默认模型失败：${String(cause)}`);
    } finally {
      setSavingDefaultKey(prev => (prev === key ? null : prev));
    }
  }

  async function toggleGroupFollow(follow: boolean) {
    setSavingDefaultKey("group_follow");
    try {
      await onToggleGroupFollowPrivate(follow);
    } catch (cause) {
      onError(`更新群聊默认策略失败：${String(cause)}`);
    } finally {
      setSavingDefaultKey(prev => (prev === "group_follow" ? null : prev));
    }
  }

  return (
    <Dialog
      title="工作台设置"
      onClose={onClose}
      wide
      className="dialog-settings-split"
    >
      <div className="settings-split">
        <nav className="settings-nav" role="tablist" aria-label="设置分区" aria-orientation="vertical">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "agents"}
            className={`settings-nav-item ${tab === "agents" ? "active" : ""}`}
            onClick={() => setTab("agents")}
          >
            <Icon name="settings" size={15} />
            <span className="settings-nav-label">成员与接入</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "defaults"}
            className={`settings-nav-item ${tab === "defaults" ? "active" : ""}`}
            onClick={() => setTab("defaults")}
          >
            <Icon name="spark" size={15} />
            <span className="settings-nav-label">默认模型</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "usage"}
            className={`settings-nav-item ${tab === "usage" ? "active" : ""}`}
            onClick={() => setTab("usage")}
          >
            <Icon name="chart" size={15} />
            <span className="settings-nav-label">用量与额度</span>
            {hasUsageWarning && <span className="settings-tab-dot" aria-label="额度提醒" />}
            <span className="settings-tab-kbd">⌘U</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "wiki"}
            className={`settings-nav-item ${tab === "wiki" ? "active" : ""}`}
            onClick={() => setTab("wiki")}
          >
            <Icon name="file" size={15} />
            <span className="settings-nav-label">知识库</span>
          </button>
        </nav>

        <div className="settings-content" role="tabpanel">
          {tab === "wiki" ? (
            <WikiSettingsPanel onError={onError} />
          ) : tab === "usage" ? (
            <UsageDashboard
              data={usageData}
              days={usageDays}
              loading={usageLoading}
              embedded
              onChangeDays={onChangeUsageDays}
              onRefresh={onRefreshUsage}
            />
          ) : tab === "defaults" ? (
            <div className="defaults-pane">
              <p className="dialog-intro settings-pane-intro">
                为私聊（含项目空间）与群聊预设各 Agent 的默认模型、思考程度与响应速度。新建会话将自动套用；在聊天页面内临时调整则仅保留于当前会话，不会覆盖全局默认。
              </p>

              <label className="defaults-follow-card" style={{ marginBottom: 12 }}>
                <div className="defaults-follow-text">
                  <strong>接近上下文上限时自动整理</strong>
                  <small>开启后，当会话上下文占用达到 80% 时自动生成阶段检查点并无感轮转底层会话（聊天历史完整保留）；关闭后仅在 70% 时提示手动整理。</small>
                </div>
                <input
                  type="checkbox"
                  className="defaults-switch"
                  checked={autoCompact}
                  disabled={savingAutoCompact}
                  onChange={e => void toggleAutoCompact(e.target.checked)}
                />
              </label>

              <div className="defaults-scope-bar" role="tablist" aria-label="默认配置场景">
                <button
                  type="button"
                  role="tab"
                  aria-selected={defaultScope === "private"}
                  className={`defaults-scope-btn ${defaultScope === "private" ? "active" : ""}`}
                  onClick={() => setDefaultScope("private")}
                >
                  <Icon name="chat" size={14} />
                  <span>私聊与项目默认</span>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={defaultScope === "group"}
                  className={`defaults-scope-btn ${defaultScope === "group" ? "active" : ""}`}
                  onClick={() => setDefaultScope("group")}
                >
                  <Icon name="group" size={14} />
                  <span>群聊默认</span>
                </button>
              </div>

              {defaultScope === "group" && (
                <label className="defaults-follow-card">
                  <div className="defaults-follow-text">
                    <strong>跟随私聊默认配置</strong>
                    <small>开启后，新建群聊会话直接沿用「私聊与项目默认」；关闭后可为群聊单独配置更轻量或更偏重协作的模型与速度。</small>
                  </div>
                  <input
                    type="checkbox"
                    className="defaults-switch"
                    checked={modelDefaults.group_follow_private}
                    disabled={savingDefaultKey === "group_follow"}
                    onChange={e => void toggleGroupFollow(e.target.checked)}
                  />
                </label>
              )}

              {defaultScope === "group" && modelDefaults.group_follow_private ? (
                <div className="defaults-inherited-note">
                  <Icon name="check" size={15} />
                  <span>当前群聊已开启「跟随私聊默认配置」，无需重复维护两套参数。如需单独配置群聊模型，请关闭上方开关。</span>
                </div>
              ) : activeMembers.length === 0 ? (
                <div className="discovery-empty">
                  暂未接入任何 Agent，请先在「成员与接入」中扫描或添加 Agent。
                </div>
              ) : (
                <div className="default-model-list" role="group" aria-label="各 Agent 默认模型配置">
                  {activeMembers.map(member => {
                    const liveProbe = probes[member.id];
                    const models = liveProbe?.models ?? [];
                    const isProbing = !!probing[member.id];
                    const entry = findDirectModelDefault(modelDefaults, defaultScope, member.id);
                    const savedModel = entry?.model ?? "";
                    const savedEffort = entry?.reasoning_effort ?? "";
                    const savedTier = entry?.service_tier ?? "default";
                    const providerDefaultModel = models.find(m => m.isDefault) ?? models[0];
                    const effectiveModel: Model | undefined =
                      (savedModel ? models.find(m => m.model === savedModel) : undefined) ??
                      providerDefaultModel;
                    const efforts = effectiveModel?.supportedReasoningEfforts ?? [];
                    const tiers = speedChoices(effectiveModel);
                    const hasFastTiers = tiers.length > 1;
                    const isSaving = savingDefaultKey === `${defaultScope}:${member.id}`;
                    const hasCustom = !!savedModel || !!savedEffort || savedTier !== "default";

                    return (
                      <div
                        className={`default-model-card ${!member.enabled ? "is-disabled" : ""}`}
                        key={`${defaultScope}:${member.id}`}
                      >
                        <div className="default-model-card-head">
                          <div className="default-model-agent">
                            <Avatar agent={member.id} />
                            <div>
                              <div className="provider-card-title">
                                <strong>{member.name}</strong>
                                {!member.enabled && (
                                  <span className="provider-status-badge tone-muted">已停用</span>
                                )}
                                {hasCustom && (
                                  <span className="provider-status-badge tone-ready">已自定义</span>
                                )}
                              </div>
                              <p className="provider-card-meta">
                                {liveProbe
                                  ? `${models.length} 个可用模型`
                                  : isProbing
                                    ? "正在检测可用模型…"
                                    : "尚未获取模型列表"}
                              </p>
                            </div>
                          </div>
                          <div className="default-model-head-actions">
                            {hasCustom && (
                              <button
                                type="button"
                                className="mcp-reset-link"
                                disabled={isSaving}
                                onClick={() =>
                                  void saveAgentDefault(defaultScope, member.id, "", null, null)
                                }
                              >
                                恢复原厂默认
                              </button>
                            )}
                            {!liveProbe && member.enabled && (
                              <button
                                type="button"
                                className="outline-button"
                                disabled={isProbing || !desktop}
                                onClick={() => onProbeAgent(member)}
                              >
                                {isProbing ? "检测中…" : "检测模型"}
                              </button>
                            )}
                          </div>
                        </div>

                        {liveProbe && models.length > 0 ? (
                          <div className="default-model-controls">
                            <div className="default-model-field">
                              <label
                                className="default-model-label"
                                htmlFor={`default-model-${defaultScope}-${member.id}`}
                              >
                                默认模型
                              </label>
                              <select
                                id={`default-model-${defaultScope}-${member.id}`}
                                className="text-input default-model-select"
                                value={savedModel}
                                disabled={isSaving}
                                onChange={e => {
                                  const nextModelId = e.target.value;
                                  const nextModelObj =
                                    (nextModelId
                                      ? models.find(m => m.model === nextModelId)
                                      : undefined) ?? providerDefaultModel;
                                  const nextEfforts = nextModelObj?.supportedReasoningEfforts ?? [];
                                  const nextEffort =
                                    savedEffort &&
                                    nextEfforts.some(c => c.reasoningEffort === savedEffort)
                                      ? savedEffort
                                      : null;
                                  const nextTiers = speedChoices(nextModelObj);
                                  const nextTier =
                                    savedTier &&
                                    savedTier !== "default" &&
                                    nextTiers.some(t => t.id === savedTier)
                                      ? savedTier
                                      : null;
                                  void saveAgentDefault(
                                    defaultScope,
                                    member.id,
                                    nextModelId,
                                    nextEffort,
                                    nextTier,
                                  );
                                }}
                              >
                                <option value="">
                                  跟随原厂推荐
                                  {providerDefaultModel
                                    ? `（${providerDefaultModel.displayName}）`
                                    : ""}
                                </option>
                                {savedModel && !models.some(m => m.model === savedModel) && (
                                  <option value={savedModel}>{savedModel}（当前列表未包含）</option>
                                )}
                                {models.map(item => (
                                  <option key={item.model} value={item.model}>
                                    {item.displayName}
                                    {item.model !== item.displayName ? ` (${item.model})` : ""}
                                    {item.isDefault ? " · 推荐" : ""}
                                  </option>
                                ))}
                              </select>
                            </div>

                            {efforts.length > 0 && (
                              <div className="default-model-field">
                                <span className="default-model-label">
                                  <Icon name="spark" size={12} />
                                  思考程度
                                </span>
                                <div className="mcp-segmented" role="radiogroup" aria-label={`${member.name} 默认思考程度`}>
                                  <button
                                    type="button"
                                    role="radio"
                                    aria-checked={!savedEffort}
                                    disabled={isSaving}
                                    className={`mcp-seg-btn ${!savedEffort ? "active" : ""}`}
                                    onClick={() =>
                                      void saveAgentDefault(
                                        defaultScope,
                                        member.id,
                                        savedModel,
                                        null,
                                        savedTier === "default" ? null : savedTier,
                                      )
                                    }
                                  >
                                    默认
                                  </button>
                                  {efforts.map(choice => {
                                    const active = savedEffort === choice.reasoningEffort;
                                    const label =
                                      effortLabels[choice.reasoningEffort] ??
                                      choice.reasoningEffort;
                                    return (
                                      <button
                                        key={choice.reasoningEffort}
                                        type="button"
                                        role="radio"
                                        aria-checked={active}
                                        disabled={isSaving}
                                        className={`mcp-seg-btn ${active ? "active" : ""}`}
                                        onClick={() =>
                                          void saveAgentDefault(
                                            defaultScope,
                                            member.id,
                                            savedModel,
                                            choice.reasoningEffort,
                                            savedTier === "default" ? null : savedTier,
                                          )
                                        }
                                      >
                                        {label}
                                      </button>
                                    );
                                  })}
                                </div>
                              </div>
                            )}

                            {hasFastTiers && (
                              <div className="default-model-field">
                                <span className="default-model-label">
                                  <Icon name="bolt" size={12} />
                                  响应速度
                                </span>
                                <div className="mcp-segmented" role="radiogroup" aria-label={`${member.name} 默认响应速度`}>
                                  {tiers.map(tier => {
                                    const active = savedTier === tier.id;
                                    const isAccel = tier.id !== "default";
                                    return (
                                      <button
                                        key={tier.id}
                                        type="button"
                                        role="radio"
                                        aria-checked={active}
                                        disabled={isSaving}
                                        className={`mcp-seg-btn ${active ? (isAccel ? "active-fast" : "active") : ""}`}
                                        onClick={() =>
                                          void saveAgentDefault(
                                            defaultScope,
                                            member.id,
                                            savedModel,
                                            savedEffort || null,
                                            tier.id === "default" ? null : tier.id,
                                          )
                                        }
                                      >
                                        {isAccel && <Icon name="bolt" size={12} />}
                                        {tier.name}
                                      </button>
                                    );
                                  })}
                                </div>
                              </div>
                            )}
                          </div>
                        ) : (
                          <p className="default-model-empty-hint">
                            {isProbing
                              ? "正在连接并拉取模型列表，请稍候…"
                              : "点击右上角「检测模型」获取该 Agent 支持的模型与推理档位。"}
                          </p>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          ) : (
            <>
              <p className="dialog-intro settings-pane-intro">
                自动并发探测本机已安装的 CLI Agent 并接入侧边栏，也支持通过一次性邀请手动添加自定义实例。
              </p>

              <div className="settings-toolbar">
                <div className="settings-toolbar-main">
                  <button className="primary" disabled={discovering || !desktop} onClick={onDiscover}>
                    {discovering ? "正在扫描本机组件…" : "重新扫描本机组件"}
                  </button>
                  <button className="outline-button" onClick={onOpenPairing}>
                    手动添加 Agent
                  </button>
                </div>
                <button
                  className={`dispatch-toggle ${paused ? "is-paused" : ""}`}
                  onClick={onToggleDispatch}
                  title={paused ? "当前所有自动派发已暂停，点击恢复" : "点击暂停全部后台派发"}
                >
                  <span className="dispatch-dot" />
                  {paused ? "已暂停派发 / 点击继续" : "派发运行中 / 暂停"}
                </button>
              </div>

              <div className="field-caption settings-section-head">
                <span>已接入成员 ({activeMembers.length})</span>
                {discovering && <span className="settings-scanning-hint">正在并行探测…</span>}
              </div>

              <div className="provider-list" role="group" aria-label="已接入成员">
                {activeMembers.length ? (
                  activeMembers.map(member => {
                    const liveProbe = probes[member.id];
                    const disc = discoveryByProvider.get(member.provider_kind);
                    const isProbing = !!probing[member.id];
                    const status = memberStatus(member, disc, isProbing);
                    const modelCount = liveProbe?.models.length ?? disc?.model_count ?? 0;
                    const version = formatVersion(liveProbe?.version ?? disc?.version ?? member.runtime_version);
                    const authLabel = liveProbe?.auth ?? disc?.auth ?? null;
                    const runtimeLabel = member.runtime_label ?? disc?.runtime_label ?? member.runtime_kind ?? member.provider_kind;
                    const caps = capabilityTags(member.capabilities ?? disc?.capabilities);
                    const metaParts: string[] = [];
                    if (authLabel) metaParts.push(authLabel);
                    if (modelCount > 0) metaParts.push(`${modelCount} 个模型`);
                    if (version) metaParts.push(version);
                    const warningText =
                      status.tone === "warn"
                        ? (disc?.auth_hint ?? member.error ?? "请检查本机运行组件或原厂登录状态。")
                        : null;

                    return (
                      <div className={`provider-card ${!member.enabled ? "is-disabled" : ""}`} key={member.id}>
                        <div className="provider-card-main">
                          <Avatar agent={member.id} />
                          <div className="provider-card-body">
                            <div className="provider-card-title">
                              <strong>{member.name}</strong>
                              <span className={`provider-status-badge tone-${status.tone}`}>{status.label}</span>
                              <span className="provider-runtime-tag">{runtimeLabel}</span>
                            </div>
                            <p className="provider-card-meta">
                              {metaParts.length ? metaParts.join(" / ") : member.description}
                            </p>
                            {caps.length > 0 && (
                              <div className="provider-caps" aria-label="支持能力">
                                {caps.map(tag => (
                                  <span className="provider-cap-pill" key={tag}>{tag}</span>
                                ))}
                              </div>
                            )}
                            {warningText && <p className="provider-card-warn">{warningText}</p>}
                          </div>
                        </div>
                        <div className="provider-card-actions">
                          {member.registration_state === "active" && member.enabled && (
                            <button
                              className="outline-button"
                              disabled={isProbing || !desktop}
                              onClick={() => onProbeAgent(member)}
                            >
                              {isProbing ? "检测中…" : "重新检测"}
                            </button>
                          )}
                          {member.registration_state === "active" && (
                            <button className="outline-button" onClick={() => void toggleEnabled(member)}>
                              {member.enabled ? "停用" : "启用"}
                            </button>
                          )}
                          <button className="outline-button danger-subtle" onClick={() => void revokeAgent(member)}>
                            移除
                          </button>
                        </div>
                      </div>
                    );
                  })
                ) : (
                  <div className="discovery-empty">
                    {discovering
                      ? "正在并行扫描本机已安装的 Agent 组件…"
                      : "暂未接入任何 Agent，请点击上方「重新扫描本机组件」或「手动添加 Agent」。"}
                  </div>
                )}
              </div>

              {unregisteredDiscoveries.length > 0 && (
                <>
                  <div className="field-caption settings-section-head">
                    <span>其他本机探测结果 ({unregisteredDiscoveries.length})</span>
                  </div>
                  <div className="discovery-list" role="group" aria-label="其他本机探测结果">
                    {unregisteredDiscoveries.map(item => {
                      const version = formatVersion(item.version);
                      return (
                        <div className="discovery-item" key={item.provider_kind}>
                          <Avatar agent={item.provider_kind} />
                          <div>
                            <div className="provider-card-title">
                              <strong>{item.display_name}</strong>
                              {item.runtime_label && <span className="provider-runtime-tag">{item.runtime_label}</span>}
                            </div>
                            <p>{item.message}{version ? ` (${version})` : ""}</p>
                          </div>
                          <span className={`discovery-status ${item.status}`}>
                            {item.status === "connected" ? "可接入" : item.status === "needs_auth" ? "需原厂登录" : "未检测到"}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                </>
              )}

              {(runtimePaths.length > 0 || info) && (
                <details className="runtime-details">
                  <summary>运行环境与本地数据位置</summary>
                  {runtimePaths.map(item => (
                    <code key={item.id}>
                      {item.label}{item.version ? ` (${item.version})` : ""}: {item.executable}
                    </code>
                  ))}
                  {info && <code>DB: {info.database_path}</code>}
                </details>
              )}
            </>
          )}
        </div>
      </div>

      <div className="dialog-footer">
        <span>AgentCrew v{info?.version ?? "0.1.6"}</span>
        <button className="primary" onClick={onClose}>
          完成<Icon name="check" size={16} />
        </button>
      </div>
    </Dialog>
  );
}
