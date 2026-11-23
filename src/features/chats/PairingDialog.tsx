import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Dialog from "../../components/Dialog";
import { desktop, refreshAgents } from "./api";

export interface Invitation {
  id: string; code: string; prompt: string; expires_at: number; state: string;
  protocol_version?: number; reason?: string | null; display_name?: string | null;
  reported_provider_kind?: string | null; reported_runtime_kind?: string | null;
  verified_provider_kind?: string | null; verified_runtime_kind?: string | null;
  runtime_version?: string | null;
}

export default function PairingDialog({ invitation, onInvitation, onClose }: {
  invitation: Invitation | null; onInvitation: (value: Invitation | null) => void; onClose: () => void;
}) {
  const [name, setName] = useState(invitation?.display_name ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [clock, setClock] = useState(Date.now());
  useEffect(() => { const timer = setInterval(() => setClock(Date.now()), 1000); return () => clearInterval(timer); }, []);
  useEffect(() => {
    if (!desktop || !invitation?.id || ["active", "failed", "revoked"].includes(invitation.state)) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const result = await invoke<Partial<Invitation>>("pairing_snapshot", { id: invitation.id });
        if (active) {
          onInvitation({ ...invitation, ...result });
          setClock(Date.now());
          if (result.state === "active") await refreshAgents();
        }
        if (active && !["active", "failed", "revoked"].includes(result.state ?? "")) timer = setTimeout(poll, 1000);
      } catch (cause) {
        if (active) { setError(String(cause)); timer = setTimeout(poll, 3000); }
      }
    };
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [invitation?.id, invitation?.state]);

  async function create() {
    setBusy(true); setError(""); setCopied(false);
    try {
      const created = await invoke<Invitation>("pairing_create", {
        displayName: name.trim(),
        previousId: invitation?.id ?? null,
      });
      onInvitation(created);
      setClock(Date.now());
      if (created.prompt) {
        await navigator.clipboard.writeText(created.prompt).then(() => setCopied(true), () => {});
      }
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }

  async function cancel() {
    setBusy(true); setError("");
    try { await invoke("pairing_cancel", { id: invitation?.id }); onInvitation(null); }
    catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }

  const expired = !!invitation && invitation.expires_at * 1000 <= clock;
  const state = expired && invitation?.state === "pending" ? "expired" : invitation?.state;
  const labels: Record<string, string> = {
    pending: "等待 Agent 识别并上报",
    validating: "收到请求，正在核验运行组件…",
    active: "已接入，可开始协作",
    failed: "接入失败",
    revoked: "邀请已取消或应用已重启，请重新生成",
    expired: "邀请已过期，请重新生成",
  };

  return <Dialog title="添加 Agent" onClose={onClose} wide>
    <p className="dialog-intro">生成一次性配对邀请并粘贴给目标 Agent，接入后将自动识别名称与图标。</p>
    {!invitation && <>
      <label className="field-label" htmlFor="agent-name">自定义名称 <span>（可选，留空将自动按接入组件命名）</span></label>
      <input
        id="agent-name"
        className="text-input"
        maxLength={40}
        value={name}
        onChange={event => setName(event.target.value)}
        onKeyDown={event => { if (event.key === "Enter" && !busy && desktop) void create(); }}
        placeholder="留空则自动识别，如 Gemini / Codex"
        autoFocus
      />
      <div className="runtime-chips">
        <span className="runtime-chip-label">已适配协议</span>
        <span className="runtime-chip">Codex</span>
        <span className="runtime-chip">DeepSeek</span>
        <span className="runtime-chip">Gemini CLI</span>
        <span className="runtime-chip muted">更多扩展中</span>
      </div>
    </>}
    {invitation && <>
      <p role="status" className="pairing-state">{labels[state ?? "pending"] ?? state}</p>
      {invitation.reported_provider_kind && <p className="field-help">Agent 自报：{invitation.reported_provider_kind} · {invitation.reported_runtime_kind}</p>}
      {invitation.verified_provider_kind && <p className="field-help">已核验：{invitation.verified_provider_kind} · {invitation.verified_runtime_kind} · {invitation.runtime_version}</p>}
      {invitation.reason && <p className="inline-error" role="alert">{invitation.reason}</p>}
      <p className="field-help">有效期至 {new Date(invitation.expires_at * 1000).toLocaleTimeString()}</p>
      <details className="runtime-details"><summary>查看配对码</summary><code>{invitation.code}</code></details>
      <details className="runtime-details"><summary>查看完整邀请</summary><pre className="pairing-prompt">{invitation.prompt}</pre></details>
    </>}
    {error && <p role="alert" className="inline-error">{error}</p>}
    <div className="dialog-footer">
      <span>{copied ? "已生成并复制到剪贴板" : "一次性有效 · 非 API Key"}</span>
      <div className="pairing-actions">
        {invitation && state !== "active" && <button className="outline-button" disabled={busy} onClick={() => void cancel()}>取消邀请</button>}
        {(!invitation || !["active", "validating"].includes(state ?? "")) && (
          <button className={invitation ? "outline-button" : "primary"} disabled={busy || !desktop} onClick={() => void create()}>
            {busy ? "正在生成…" : invitation ? "重新生成" : "生成并复制邀请"}
          </button>
        )}
        {invitation && state === "pending" && (
          <button className="primary" onClick={() => void navigator.clipboard.writeText(invitation.prompt).then(() => setCopied(true), () => setError("复制失败，请展开邀请手动复制。"))}>
            {copied ? "已复制" : "复制邀请"}
          </button>
        )}
        {state === "active" && <button className="primary" onClick={() => { onInvitation(null); onClose(); }}>完成</button>}
      </div>
    </div>
  </Dialog>;
}
