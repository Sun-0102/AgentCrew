import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import Icon from "../../components/Icon";
import {
  addLocalWikiSource,
  getWikiSettings,
  listWikiHabits,
  listWikiSources,
  reindexWikiSource,
  removeWikiSource,
  saveWikiSettings,
  searchWiki,
  setWikiHabit,
  syncWikiProjects,
  type WikiHabitPage,
  type WikiSearchHit,
  type WikiSettings,
  type WikiSource,
} from "./api";

interface Props {
  onError: (message: string) => void;
}

function statusLabel(state: string): { text: string; color: string } {
  if (state === "ready") return { text: "已就绪", color: "#16a34a" };
  if (state === "indexing") return { text: "索引中…", color: "#d97706" };
  if (state === "error") return { text: "目录不可用", color: "#dc2626" };
  return { text: "待索引", color: "var(--muted, #64748b)" };
}

export default function WikiSettingsPanel({ onError }: Props) {
  const [settings, setSettings] = useState<WikiSettings | null>(null);
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [habits, setHabits] = useState<WikiHabitPage[]>([]);
  const [loading, setLoading] = useState(true);
  const [syncing, setSyncing] = useState(false);
  const [savingAdvanced, setSavingAdvanced] = useState(false);
  const [reindexingId, setReindexingId] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ text: string; tone: "ok" | "error" } | null>(null);

  // Advanced folder tier
  const [newFolderTier, setNewFolderTier] = useState<"core_wiki" | "archive">("core_wiki");

  // Habit picker: only show dropdown when user actively searches
  const [habitQuery, setHabitQuery] = useState("");
  const [habitCandidates, setHabitCandidates] = useState<WikiSearchHit[]>([]);
  const [searchingHabits, setSearchingHabits] = useState(false);

  async function refreshAll() {
    setLoading(true);
    try {
      const [s, src, h] = await Promise.all([
        getWikiSettings(),
        listWikiSources(),
        listWikiHabits(),
      ]);
      setSettings(s);
      setSources(src);
      setHabits(h);
    } catch (cause) {
      onError(`读取知识库配置失败：${String(cause)}`);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refreshAll();
  }, []);

  useEffect(() => {
    const q = habitQuery.trim();
    if (!sources.length || !q) {
      setHabitCandidates([]);
      setSearchingHabits(false);
      return;
    }
    let cancelled = false;
    setSearchingHabits(true);
    const timer = window.setTimeout(() => {
      void searchWiki({ query: q, limit: 8 })
        .then(hits => {
          if (!cancelled) setHabitCandidates(hits);
        })
        .catch(() => {
          if (!cancelled) setHabitCandidates([]);
        })
        .finally(() => {
          if (!cancelled) setSearchingHabits(false);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [habitQuery, sources.length]);

  const totalHabitBytes = useMemo(
    () => habits.filter(h => h.enabled).reduce((sum, item) => sum + item.size_bytes, 0),
    [habits],
  );
  const habitBudget = settings?.habits_budget_bytes ?? 1800;

  async function handleToggleEnabled(enabled: boolean) {
    if (!settings) return;
    const next = { ...settings, enabled };
    setSettings(next);
    try {
      const saved = await saveWikiSettings(next);
      setSettings(saved);
    } catch (cause) {
      onError(`更新开关失败：${String(cause)}`);
    }
  }

  async function handleConnectAndSync() {
    if (!settings) return;
    setSyncing(true);
    setNotice(null);
    try {
      const saved = await saveWikiSettings(settings);
      setSettings(saved);
      const updated = await syncWikiProjects();
      setSources(updated);
      setNotice({
        text: updated.length
          ? `连接成功，已同步 ${updated.length} 个知识库。`
          : "已连接 LLM Wiki，但暂无项目，也可以直接添加本地文件夹。",
        tone: "ok",
      });
    } catch (cause) {
      const msg = String(cause).replace(/^Error:\s*/, "");
      setNotice({ text: msg, tone: "error" });
    } finally {
      setSyncing(false);
    }
  }

  async function handleSaveAdvanced() {
    if (!settings) return;
    setSavingAdvanced(true);
    setNotice(null);
    try {
      const saved = await saveWikiSettings(settings);
      setSettings(saved);
      setNotice({ text: "高级设置已保存。", tone: "ok" });
    } catch (cause) {
      onError(`保存失败：${String(cause)}`);
    } finally {
      setSavingAdvanced(false);
    }
  }

  async function handleAddLocalFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择本地知识库文件夹",
      });
      if (!selected || typeof selected !== "string") return;
      setReindexingId("__adding__");
      setNotice(null);
      const added = await addLocalWikiSource({
        rootPath: selected,
        tier: newFolderTier,
      });
      const updated = await listWikiSources();
      setSources(updated);
      setNotice({ text: `已添加「${added.name}」（${added.doc_count} 篇有效文档）。`, tone: "ok" });
    } catch (cause) {
      onError(`添加本地文件夹失败：${String(cause)}`);
    } finally {
      setReindexingId(null);
    }
  }

  async function handleReindex(sourceId: string) {
    setReindexingId(sourceId);
    setNotice(null);
    try {
      const stats = await reindexWikiSource(sourceId, true);
      const updated = await listWikiSources();
      setSources(updated);
      setNotice({ text: `刷新完成，共 ${stats.active_docs} 篇有效文档。`, tone: "ok" });
    } catch (cause) {
      onError(`刷新索引失败：${String(cause)}`);
    } finally {
      setReindexingId(null);
    }
  }

  async function handleRemoveSource(source: WikiSource) {
    try {
      await removeWikiSource(source.id);
      setSources(prev => prev.filter(item => item.id !== source.id));
      setHabits(prev => prev.filter(item => item.source_id !== source.id));
      setNotice({ text: `已移除「${source.name}」（不会删除原文件）。`, tone: "ok" });
    } catch (cause) {
      onError(`移除失败：${String(cause)}`);
    }
  }

  async function toggleHabit(sourceId: string, relPath: string, title: string, enabled: boolean) {
    try {
      const next = await setWikiHabit({ sourceId, relPath, title, enabled });
      setHabits(next);
      if (enabled) setHabitQuery("");
    } catch (cause) {
      onError(`更新偏好笔记失败：${String(cause)}`);
    }
  }

  if (loading || !settings) {
    return <p className="field-help">正在加载知识库…</p>;
  }

  const habitKeySet = new Set(habits.filter(h => h.enabled).map(h => `${h.source_id}:${h.rel_path}`));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      {/* 1. 我的知识库 */}
      <div style={{ padding: 16, borderRadius: 10, border: "1px solid var(--border)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, marginBottom: 12 }}>
          <div>
            <strong style={{ fontSize: 15 }}>我的知识库</strong>
            <p className="field-help" style={{ margin: "2px 0 0" }}>
              接入本地文件夹或同步 LLM Wiki，群聊绑定后 AI 会按需查阅，不消耗额外 API 费用。
            </p>
          </div>
          <label style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: "pointer", fontSize: 13, flexShrink: 0 }}>
            <input
              type="checkbox"
              checked={settings.enabled}
              onChange={e => void handleToggleEnabled(e.target.checked)}
            />
            <span>启用</span>
          </label>
        </div>

        {/* 一步连接同步条 */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 12 }}>
          <input
            type="password"
            className="text-input"
            style={{ flex: 1, minWidth: 200 }}
            value={settings.api_token}
            onChange={e => setSettings({ ...settings, api_token: e.target.value })}
            placeholder="LLM Wiki API Token（如未开启鉴权可留空）"
          />
          <button
            type="button"
            className="primary"
            disabled={syncing}
            onClick={() => void handleConnectAndSync()}
          >
            {syncing ? "同步中…" : "连接并同步"}
          </button>
          <button
            type="button"
            className="outline-button"
            disabled={reindexingId === "__adding__"}
            onClick={() => void handleAddLocalFolder()}
          >
            <Icon name="plus" size={14} />
            <span>{reindexingId === "__adding__" ? "添加中…" : "添加本地文件夹"}</span>
          </button>
        </div>

        {notice && (
          <p
            className="field-help"
            style={{
              margin: "0 0 10px",
              color: notice.tone === "error" ? "var(--danger, #dc2626)" : "var(--accent)",
            }}
          >
            {notice.text}
          </p>
        )}

        {!sources.length ? (
          <div
            style={{
              padding: "16px 12px",
              borderRadius: 8,
              border: "1px dashed var(--border)",
              textAlign: "center",
              fontSize: 13,
              opacity: 0.75,
            }}
          >
            还没有知识库。点击上方「连接并同步」或「添加本地文件夹」开始使用。
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {sources.map(src => {
              const st = statusLabel(src.index_state);
              return (
                <div
                  key={src.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    padding: "10px 12px",
                    borderRadius: 8,
                    border: "1px solid var(--border)",
                  }}
                >
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                      <strong>{src.name}</strong>
                      <span style={{ fontSize: 12, color: st.color, display: "inline-flex", alignItems: "center", gap: 4 }}>
                        <span style={{ width: 6, height: 6, borderRadius: "50%", background: st.color, display: "inline-block" }} />
                        {st.text} · {src.doc_count} 篇
                      </span>
                    </div>
                    <div
                      style={{
                        fontSize: 12,
                        opacity: 0.65,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        marginTop: 2,
                      }}
                    >
                      {src.root_path}
                    </div>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, flexShrink: 0 }}>
                    <button
                      type="button"
                      className="outline-button"
                      disabled={reindexingId === src.id}
                      onClick={() => void handleReindex(src.id)}
                    >
                      {reindexingId === src.id ? "刷新中…" : "刷新"}
                    </button>
                    <button
                      type="button"
                      className="outline-button danger-button"
                      onClick={() => void handleRemoveSource(src)}
                    >
                      移除
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* 2. 个人偏好笔记（可选） */}
      <div style={{ padding: 16, borderRadius: 10, border: "1px solid var(--border)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 4 }}>
          <strong style={{ fontSize: 14 }}>个人偏好笔记（可选）</strong>
          {habits.length > 0 && (
            <span style={{ fontSize: 12, opacity: 0.7 }}>
              已选 {habits.length} 篇{totalHabitBytes > habitBudget ? "（超出推荐长度，将自动精简）" : ""}
            </span>
          )}
        </div>
        <p className="field-help" style={{ margin: "0 0 10px" }}>
          选 1～2 篇写有你汇报风格或格式要求的笔记，AI 每次对话都会记住；业务资料无需在此勾选。
        </p>

        {habits.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 10 }}>
            {habits.map(h => (
              <span
                key={`${h.source_id}:${h.rel_path}`}
                className="label-chip"
                style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "4px 10px", fontSize: 12 }}
              >
                <span>{h.title}</span>
                <button
                  type="button"
                  style={{ border: "none", background: "transparent", cursor: "pointer", padding: 0, color: "inherit", fontSize: 14, lineHeight: 1 }}
                  onClick={() => void toggleHabit(h.source_id, h.rel_path, h.title, false)}
                  title="移除"
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}

        <input
          className="text-input"
          value={habitQuery}
          onChange={e => setHabitQuery(e.target.value)}
          placeholder={sources.length ? "输入关键词搜索笔记添加（如：习惯、风格）…" : "请先在上方添加知识库"}
          disabled={!sources.length}
        />

        {habitQuery.trim().length > 0 && (
          <div style={{ marginTop: 8, maxHeight: 160, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
            {searchingHabits ? (
              <span className="field-help">搜索中…</span>
            ) : !habitCandidates.length ? (
              <span className="field-help">未找到匹配的笔记。</span>
            ) : (
              habitCandidates.map(hit => {
                const key = `${hit.source_id}:${hit.rel_path}`;
                const checked = habitKeySet.has(key);
                return (
                  <button
                    key={key}
                    type="button"
                    onClick={() => void toggleHabit(hit.source_id, hit.rel_path, hit.title, !checked)}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 8,
                      padding: "7px 10px",
                      borderRadius: 6,
                      cursor: "pointer",
                      textAlign: "left",
                      border: checked ? "1px solid var(--accent)" : "1px solid var(--border)",
                      background: "transparent",
                    }}
                  >
                    <span style={{ fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      <strong>{hit.title}</strong>
                      <span style={{ opacity: 0.6 }}> · {hit.rel_path}</span>
                    </span>
                    <span style={{ fontSize: 12, opacity: 0.75, flexShrink: 0 }}>
                      {checked ? "已添加 ✓" : "+ 添加"}
                    </span>
                  </button>
                );
              })
            )}
          </div>
        )}
      </div>

      {/* 3. 高级设置（默认折叠） */}
      <details style={{ padding: "10px 14px", borderRadius: 10, border: "1px solid var(--border)" }}>
        <summary style={{ cursor: "pointer", fontSize: 13, fontWeight: 500, userSelect: "none" }}>
          高级设置（服务地址、上下文长度上限、目录分层）
        </summary>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 12 }}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
            <div>
              <label className="field-label" htmlFor="wiki-base-url">LLM Wiki 本机地址</label>
              <input
                id="wiki-base-url"
                className="text-input"
                value={settings.base_url}
                onChange={e => setSettings({ ...settings, base_url: e.target.value })}
                placeholder="http://127.0.0.1:19828"
              />
            </div>
            <div>
              <label className="field-label" htmlFor="wiki-folder-tier">新添本地目录层级</label>
              <select
                id="wiki-folder-tier"
                className="text-input"
                value={newFolderTier}
                onChange={e => setNewFolderTier(e.target.value as "core_wiki" | "archive")}
              >
                <option value="core_wiki">核心业务规则（默认）</option>
                <option value="archive">海量历史档案（仅显式开启时检索）</option>
              </select>
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10 }}>
            <div>
              <label className="field-label" htmlFor="wiki-habits-budget">偏好笔记上限 (字节)</label>
              <input
                id="wiki-habits-budget"
                type="number"
                min={200}
                max={12000}
                className="text-input"
                value={settings.habits_budget_bytes}
                onChange={e => setSettings({ ...settings, habits_budget_bytes: Number(e.target.value) || 1800 })}
              />
            </div>
            <div>
              <label className="field-label" htmlFor="wiki-l0-budget">目录概览上限 (字节)</label>
              <input
                id="wiki-l0-budget"
                type="number"
                min={200}
                max={4000}
                className="text-input"
                value={settings.l0_map_budget_bytes}
                onChange={e => setSettings({ ...settings, l0_map_budget_bytes: Number(e.target.value) || 800 })}
              />
            </div>
            <div>
              <label className="field-label" htmlFor="wiki-read-budget">单篇精读上限 (字节)</label>
              <input
                id="wiki-read-budget"
                type="number"
                min={1000}
                max={24000}
                className="text-input"
                value={settings.read_budget_bytes}
                onChange={e => setSettings({ ...settings, read_budget_bytes: Number(e.target.value) || 6000 })}
              />
            </div>
          </div>

          <div>
            <button type="button" className="outline-button" disabled={savingAdvanced} onClick={() => void handleSaveAdvanced()}>
              {savingAdvanced ? "保存中…" : "保存高级设置"}
            </button>
          </div>
        </div>
      </details>
    </div>
  );
}
