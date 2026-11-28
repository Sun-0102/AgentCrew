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
  testWikiConnection,
  type WikiConnectionTest,
  type WikiHabitPage,
  type WikiSearchHit,
  type WikiSettings,
  type WikiSource,
} from "./api";

interface Props {
  onError: (message: string) => void;
}

export default function WikiSettingsPanel({ onError }: Props) {
  const [settings, setSettings] = useState<WikiSettings | null>(null);
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [habits, setHabits] = useState<WikiHabitPage[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [reindexingId, setReindexingId] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<WikiConnectionTest | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Add local folder tier selection
  const [newFolderTier, setNewFolderTier] = useState<"core_wiki" | "archive">("core_wiki");

  // Habit page picker search
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
    if (!sources.length) {
      setHabitCandidates([]);
      return;
    }
    let cancelled = false;
    setSearchingHabits(true);
    void searchWiki({ query: habitQuery, limit: 12 })
      .then(hits => {
        if (!cancelled) setHabitCandidates(hits);
      })
      .catch(() => {
        if (!cancelled) setHabitCandidates([]);
      })
      .finally(() => {
        if (!cancelled) setSearchingHabits(false);
      });
    return () => {
      cancelled = true;
    };
  }, [habitQuery, sources.length]);

  const totalHabitBytes = useMemo(
    () => habits.filter(h => h.enabled).reduce((sum, item) => sum + item.size_bytes, 0),
    [habits],
  );
  const habitBudget = settings?.habits_budget_bytes ?? 1800;
  const habitPercent = Math.min(100, Math.round((totalHabitBytes / Math.max(1, habitBudget)) * 100));

  async function handleSaveSettings(next?: WikiSettings) {
    const target = next ?? settings;
    if (!target) return;
    setSaving(true);
    setNotice(null);
    try {
      const saved = await saveWikiSettings(target);
      setSettings(saved);
      setNotice("知识库连接与预算配置已保存。");
    } catch (cause) {
      onError(`保存知识库配置失败：${String(cause)}`);
    } finally {
      setSaving(false);
    }
  }

  async function handleTestConnection() {
    if (!settings) return;
    setTesting(true);
    setNotice(null);
    try {
      const res = await testWikiConnection(settings.base_url, settings.api_token);
      setTestResult(res);
    } catch (cause) {
      onError(`测试连接失败：${String(cause)}`);
    } finally {
      setTesting(false);
    }
  }

  async function handleSyncProjects() {
    if (!settings) return;
    setSyncing(true);
    setNotice(null);
    try {
      await saveWikiSettings(settings);
      const updated = await syncWikiProjects();
      setSources(updated);
      setNotice(`已同步 LLM Wiki 项目，当前共 ${updated.length} 个知识源。`);
    } catch (cause) {
      onError(`同步 LLM Wiki 项目失败：${String(cause)}`);
    } finally {
      setSyncing(false);
    }
  }

  async function handleAddLocalFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择本地 Markdown / LLM Wiki 知识库目录",
      });
      if (!selected || typeof selected !== "string") return;
      setReindexingId("__adding__");
      const added = await addLocalWikiSource({
        rootPath: selected,
        tier: newFolderTier,
      });
      const updated = await listWikiSources();
      setSources(updated);
      setNotice(`已接入本地目录「${added.name}」，索引有效文档 ${added.doc_count} 篇。`);
    } catch (cause) {
      onError(`接入本地目录失败：${String(cause)}`);
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
      setNotice(`索引完成：扫描 ${stats.scanned} 个文件，更新 ${stats.indexed} 篇，有效文档 ${stats.active_docs} 篇。`);
    } catch (cause) {
      onError(`重建索引失败：${String(cause)}`);
    } finally {
      setReindexingId(null);
    }
  }

  async function handleRemoveSource(source: WikiSource) {
    try {
      await removeWikiSource(source.id);
      setSources(prev => prev.filter(item => item.id !== source.id));
      setHabits(prev => prev.filter(item => item.source_id !== source.id));
      setNotice(`已移除知识源「${source.name}」（未删除本机原文件）。`);
    } catch (cause) {
      onError(`移除知识源失败：${String(cause)}`);
    }
  }

  async function toggleHabit(sourceId: string, relPath: string, title: string, enabled: boolean) {
    try {
      const next = await setWikiHabit({ sourceId, relPath, title, enabled });
      setHabits(next);
    } catch (cause) {
      onError(`更新个人习惯页失败：${String(cause)}`);
    }
  }

  if (loading || !settings) {
    return <p className="field-help">正在加载知识库配置…</p>;
  }

  const habitKeySet = new Set(habits.filter(h => h.enabled).map(h => `${h.source_id}:${h.rel_path}`));

  return (
    <div className="settings-section-stack" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div className="settings-card" style={{ padding: 14, borderRadius: 10, border: "1px solid var(--border)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
          <div>
            <strong>LLM Wiki 本地连接（0 额外 API 费用）</strong>
            <p className="field-help" style={{ margin: "2px 0 0" }}>
              仅访问本机只读文件/图谱端点，不调用 /chat；服务未运行时自动离线只读本地目录。
            </p>
          </div>
          <label style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: "pointer", fontSize: 13 }}>
            <input
              type="checkbox"
              checked={settings.enabled}
              onChange={e => {
                const next = { ...settings, enabled: e.target.checked };
                setSettings(next);
                void handleSaveSettings(next);
              }}
            />
            <span>启用知识库</span>
          </label>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
          <div>
            <label className="field-label" htmlFor="wiki-base-url">本机服务地址</label>
            <input
              id="wiki-base-url"
              className="text-input"
              value={settings.base_url}
              onChange={e => setSettings({ ...settings, base_url: e.target.value })}
              placeholder="http://127.0.0.1:19828"
            />
          </div>
          <div>
            <label className="field-label" htmlFor="wiki-api-token">API Token（可选）</label>
            <input
              id="wiki-api-token"
              type="password"
              className="text-input"
              value={settings.api_token}
              onChange={e => setSettings({ ...settings, api_token: e.target.value })}
              placeholder="Bearer Token（本机免鉴权可留空）"
            />
          </div>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10, marginTop: 10 }}>
          <div>
            <label className="field-label" htmlFor="wiki-habits-budget">个人习惯预算 (字节)</label>
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
            <label className="field-label" htmlFor="wiki-l0-budget">L0 概览卡预算 (字节)</label>
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
            <label className="field-label" htmlFor="wiki-read-budget">L2 精读预算 (字节)</label>
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

        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 12, flexWrap: "wrap" }}>
          <button type="button" className="primary" disabled={saving} onClick={() => void handleSaveSettings()}>
            {saving ? "保存中…" : "保存配置"}
          </button>
          <button type="button" className="outline-button" disabled={testing} onClick={() => void handleTestConnection()}>
            {testing ? "检测中…" : "测试连接"}
          </button>
          <button type="button" className="outline-button" disabled={syncing} onClick={() => void handleSyncProjects()}>
            {syncing ? "同步中…" : "同步 LLM Wiki 项目"}
          </button>
        </div>

        {testResult && (
          <p className="field-help" style={{ marginTop: 8, color: testResult.ok ? "var(--accent)" : "var(--danger)" }}>
            {testResult.message}
          </p>
        )}
        {notice && (
          <p className="field-help" style={{ marginTop: 8, color: "var(--accent)" }}>
            {notice}
          </p>
        )}
      </div>

      <div className="settings-card" style={{ padding: 14, borderRadius: 10, border: "1px solid var(--border)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, flexWrap: "wrap", marginBottom: 10 }}>
          <div>
            <strong>已接入知识源（本地 FTS5 毫秒索引）</strong>
            <p className="field-help" style={{ margin: "2px 0 0" }}>
              自动排除 raw/ 与 status: deprecated / #已废弃 旧规；支持万级文件增量更新。
            </p>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <select
              className="text-input"
              style={{ width: "auto", padding: "4px 8px", fontSize: 12 }}
              value={newFolderTier}
              onChange={e => setNewFolderTier(e.target.value as "core_wiki" | "archive")}
            >
              <option value="core_wiki">核心规则层 (core_wiki)</option>
              <option value="archive">海量档案层 (archive)</option>
            </select>
            <button
              type="button"
              className="outline-button"
              disabled={reindexingId === "__adding__"}
              onClick={() => void handleAddLocalFolder()}
            >
              <Icon name="plus" size={14} />
              <span>{reindexingId === "__adding__" ? "索引中…" : "添加本地目录"}</span>
            </button>
          </div>
        </div>

        {!sources.length ? (
          <p className="field-help">尚未接入任何知识库。可点击「同步 LLM Wiki 项目」或「添加本地目录」（如 mock-business-wiki）。</p>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {sources.map(src => (
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
                  background: "var(--surface-2, transparent)",
                }}
              >
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                    <strong>{src.name}</strong>
                    <span className="label-chip">{src.source_kind === "llm_wiki" ? "LLM Wiki" : "本地目录"}</span>
                    <span className="label-chip">{src.tier === "archive" ? "档案层" : "核心层"}</span>
                    <span style={{ fontSize: 12, opacity: 0.75 }}>
                      有效文档 {src.doc_count} 篇 · 状态：{src.index_state}
                    </span>
                  </div>
                  <div style={{ fontSize: 12, opacity: 0.7, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", marginTop: 2 }}>
                    {src.root_path}{src.content_subdir ? `/${src.content_subdir}` : ""}
                  </div>
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <button
                    type="button"
                    className="outline-button"
                    disabled={reindexingId === src.id}
                    onClick={() => void handleReindex(src.id)}
                  >
                    {reindexingId === src.id ? "索引中…" : "重建索引"}
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
            ))}
          </div>
        )}
      </div>

      <div className="settings-card" style={{ padding: 14, borderRadius: 10, border: "1px solid var(--border)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 6 }}>
          <strong>全局常驻「个人通用习惯」</strong>
          <span style={{ fontSize: 12, color: totalHabitBytes > habitBudget ? "var(--danger)" : "inherit" }}>
            已占 {totalHabitBytes} / {habitBudget} 字节 ({habitPercent}%)
          </span>
        </div>
        <div style={{ height: 6, borderRadius: 999, background: "var(--border)", overflow: "hidden", marginBottom: 8 }}>
          <div
            style={{
              height: "100%",
              width: `${habitPercent}%`,
              background: totalHabitBytes > habitBudget ? "var(--danger)" : "var(--accent)",
              transition: "width 150ms ease",
            }}
          />
        </div>
        <p className="field-help" style={{ marginBottom: 10 }}>
          仅勾选跨业务通用的汇报结构、小数精度或禁用词规范（如 owner-working-habits.md），切勿混入具体业务政策。
        </p>

        {habits.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 10 }}>
            {habits.map(h => (
              <span
                key={`${h.source_id}:${h.rel_path}`}
                className="label-chip"
                style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "4px 8px" }}
              >
                <span>{h.title} ({h.size_bytes}B)</span>
                <button
                  type="button"
                  style={{ border: "none", background: "transparent", cursor: "pointer", padding: 0, color: "inherit" }}
                  onClick={() => void toggleHabit(h.source_id, h.rel_path, h.title, false)}
                  title="取消常驻"
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
          placeholder="搜索已索引文档以勾选为常驻个人习惯…"
          disabled={!sources.length}
        />
        {sources.length > 0 && (
          <div style={{ marginTop: 8, maxHeight: 160, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
            {searchingHabits ? (
              <span className="field-help">搜索中…</span>
            ) : (
              habitCandidates.map(hit => {
                const key = `${hit.source_id}:${hit.rel_path}`;
                const checked = habitKeySet.has(key);
                return (
                  <label
                    key={key}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 8,
                      padding: "6px 8px",
                      borderRadius: 6,
                      cursor: "pointer",
                      border: "1px solid var(--border)",
                    }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={e => void toggleHabit(hit.source_id, hit.rel_path, hit.title, e.target.checked)}
                      />
                      <span style={{ fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        <strong>{hit.title}</strong> <span style={{ opacity: 0.65 }}>· {hit.rel_path}</span>
                      </span>
                    </div>
                    <span className="label-chip">{hit.source_name}</span>
                  </label>
                );
              })
            )}
          </div>
        )}
      </div>
    </div>
  );
}
