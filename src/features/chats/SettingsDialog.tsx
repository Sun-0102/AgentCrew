import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Dialog from "../../components/Dialog";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import type { AppInfo } from "../projects/api";
import UsageDashboard from "../usage/UsageDashboard";
import type { UsageDashboardData } from "../usage/api";
import { desktop, refreshAgents, type Agent, type DiscoveryItem, type Probe, type ProviderCapabilities } from "./api";

export type SettingsTab = "agents" | "usage";

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
            aria-selected={tab === "usage"}
            className={`settings-nav-item ${tab === "usage" ? "active" : ""}`}
            onClick={() => setTab("usage")}
          >
            <Icon name="chart" size={15} />
            <span className="settings-nav-label">用量与额度</span>
            {hasUsageWarning && <span className="settings-tab-dot" aria-label="额度提醒" />}
            <span className="settings-tab-kbd">⌘U</span>
          </button>
        </nav>

        <div className="settings-content" role="tabpanel">
          {tab === "usage" ? (
            <UsageDashboard
              data={usageData}
              days={usageDays}
              loading={usageLoading}
              embedded
              onChangeDays={onChangeUsageDays}
              onRefresh={onRefreshUsage}
            />
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
