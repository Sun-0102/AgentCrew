import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/Icon";
import Avatar from "../../components/Avatar";
import MessageBody from "./MessageBody";
import { agents, clock, desktop, running, statusLabels, type Group, type Message, type Probe, type Scope, type Snapshot } from "./api";
import type { Project } from "../projects/api";

interface Props {
  scope: Scope; scopeKey: string; title: string; group?: Group; project?: Project; agent?: "codex" | "deepseek";
  snapshot: Snapshot; probe: Probe | null; model: string; connecting: boolean;
  onModel: (model: string) => void; onConnect: () => void; onInvite: () => void; onSettings: () => void;
  onRefresh: () => Promise<void>; notify: (text: string) => void;
}
interface Attachment { name: string; text: string; size: number }
interface Reply { author: string; text: string }
const drafts = new Map<string, { text: string; files: Attachment[]; reply: Reply | null }>();

export default function ChatStage(p: Props) {
  const { scope, scopeKey, title, group, project, agent = "codex", snapshot, probe, model, connecting, onModel, onConnect, onInvite, onSettings, onRefresh, notify } = p;
  const [text, setText] = useState(() => drafts.get(scopeKey)?.text ?? localStorage.getItem(`ai-gent.draft.${scopeKey}`) ?? "");
  const [files, setFiles] = useState<Attachment[]>(() => drafts.get(scopeKey)?.files ?? []);
  const [reply, setReply] = useState<Reply | null>(() => drafts.get(scopeKey)?.reply ?? null);
  const [sending, setSending] = useState(false);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<"members" | "evidence" | null>(null);
  const [mentions, setMentions] = useState(false);
  const [target, setTarget] = useState<string>(agent);
  const feed = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const nearBottom = useRef(true);
  const members = group?.members ?? [agent];
  const busy = sending || running(snapshot.session?.status);
  const ready = probe && (!probe.requires_auth || probe.auth) && model && members.includes(target) && (group ? target === "codex" : target === agent);
  const tools = snapshot.messages.filter(m => m.role === "tool");
  const visibleMessages = snapshot.messages.filter((m, i, all) => m.role !== "system" || all[i + 1]?.role !== "system" || all[i + 1].text !== m.text);
  const hasMessages = snapshot.messages.some(m => !["tool", "system"].includes(m.role));
  useEffect(() => {
    drafts.set(scopeKey, { text, files, reply });
    try { if (text) localStorage.setItem(`ai-gent.draft.${scopeKey}`, text); else localStorage.removeItem(`ai-gent.draft.${scopeKey}`); }
    catch { setError("草稿未能保存，请检查本机存储空间。"); }
  }, [scopeKey, text, files, reply]);
  useEffect(() => { if (feed.current && (!hasMessages || nearBottom.current)) feed.current.scrollTop = hasMessages ? feed.current.scrollHeight : 0; }, [hasMessages, snapshot.messages.map(m => m.text).join("")]);
  useEffect(() => { if (!members.includes(target)) setTarget(members[0] ?? "codex"); }, [members.join(","), target]);
  async function copy(value: string) { try { await navigator.clipboard.writeText(value); notify("已复制消息"); } catch { notify("复制失败，请选择文字后手动复制"); } }
  async function attach(event: ChangeEvent<HTMLInputElement>) {
    try {
      const items = Array.from(event.target.files ?? []);
      if (files.length + items.length > 3) throw new Error("一次最多添加 3 个文本附件。");
      if ([...files, ...items].reduce((sum, file) => sum + file.size, 0) > 60000) throw new Error("文本附件合计请控制在 60 KB 内。");
      const incoming = await Promise.all(items.map(async file => ({ name: file.name, size: file.size, text: await file.text() })));
      setFiles(current => [...current, ...incoming]); setError(null);
    } catch (cause) { setError(String(cause)); }
    event.target.value = "";
  }
  async function send() {
    if (!text.trim() || busy || !ready) return;
    if (!desktop) { notify("请在桌面版中发送消息，浏览器预览不会调用 Agent"); return; }
    const targetAgent = text.trimStart().match(/^@(Codex|DeepSeek)\b/i)?.[1].toLowerCase() ?? target;
    if (!members.includes(targetAgent)) { setError("这个 Agent 不在当前群里，请先邀请成员。"); return; }
    if (group && targetAgent !== "codex") { setError("DeepSeek 目前仅支持独立私聊，群聊执行暂未开放。"); return; }
    let content = text.trim();
    if (reply) content = `> 回复 ${reply.author}：${reply.text.replace(/\n/g, " ").slice(0, 500)}\n\n${content}`;
    for (const file of files) content += `\n\n文本附件：${file.name}\n\`\`\`\n${file.text}\n\`\`\``;
    setSending(true); setError(null); nearBottom.current = true;
    try { await invoke(agent === "deepseek" ? "deepseek_send" : "codex_send", { ...scope, model, text: content, targetAgent }); setText(""); setFiles([]); setReply(null); await onRefresh(); }
    catch (cause) { setError(String(cause)); } finally { setSending(false); }
  }
  async function action(command: string, args: Record<string, unknown> = {}) {
    setActing(true); setError(null);
    try { await invoke(command, { ...scope, ...args }); await onRefresh(); }
    catch (cause) { setError(String(cause)); } finally { setActing(false); }
  }
  function quote(message: Message) { setReply({ author: message.role === "user" ? "我" : agent === "deepseek" ? "DeepSeek" : "Codex", text: message.text }); input.current?.focus(); }
  return <section className="chat-stage" aria-label={title}>
    <header className="chat-header"><div className="chat-title"><Avatar agent={group ? "group" : agent} /><div><h1 title={title}>{title}</h1><p><span className={`presence-dot ${ready ? "connected" : ""}`} />{group ? project ? `关联项目 · ${project.name}` : "本机协作群 · 独立上下文" : project ? project.name : "独立私聊 · 无需选择项目"}</p></div></div><div className="chat-header-actions"><button className={`members-trigger ${panel === "members" ? "selected" : ""}`} onClick={() => setPanel(panel === "members" ? null : "members")} aria-label="查看聊天成员"><span className="stacked-avatars">{members.map(id => <Avatar agent={id} small key={id} />)}</span><span>{members.length} 位 Agent<span className="members-human"> · 你</span></span><Icon name="down" size={14} /></button>{group && <button className="outline-button invite-button" onClick={onInvite}><Icon name="plus" size={17} />邀请</button>}<button className={`icon-button ${panel === "evidence" ? "selected" : ""}`} onClick={() => setPanel(panel === "evidence" ? null : "evidence")} aria-label="查看执行记录" title="执行记录"><Icon name="panel" /></button><button className="icon-button" aria-label="连接与设置" onClick={onSettings}><Icon name="more" /></button></div></header>
    <div className={`chat-stage-body ${panel ? "with-panel" : ""}`}><div className="chat-main">
      <div className="message-feed" ref={feed} onScroll={event => { const el = event.currentTarget; nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }} role="log" aria-label="聊天消息">
        {!hasMessages && <div className="conversation-welcome"><div className={`welcome-mark ${group ? "group-mark" : ""}`}>{group ? <Icon name="group" size={37} /> : <Icon name="spark" size={37} />}</div><h2>{group ? "围绕一个目标，一起讨论。" : "今天，想一起完成什么？"}</h2><p>{group ? "说说你的目标，或 @ 一位成员开始。群聊与私聊分别保留记录。" : `讨论一个想法，写一段代码，或把手头的事情交给 ${agent === "deepseek" ? "DeepSeek" : "Codex"}。`}</p><div className="prompt-suggestions">{(group ? ["一起梳理这个需求", "帮我检查一下方案"] : ["帮我理清一个想法", "一起开始一个新任务", "帮我检查一段代码"]).map(prompt => <button key={prompt} onClick={() => { setText(prompt); input.current?.focus(); }}>{prompt}<Icon name="right" size={14} /></button>)}</div></div>}
        {visibleMessages.map(message => message.role === "system" ? <div className="system-message" key={message.id}><span />{message.text}<span /></div> : message.role === "tool" ? <details className="tool-record" key={message.id}><summary><Icon name="terminal" size={15} /><span>工具执行记录</span><Icon name="down" size={14} /></summary><pre>{message.text}</pre></details> : <article className={`message-row ${message.role === "user" ? "outgoing" : "incoming"}`} key={message.id}><Avatar agent={message.role === "user" ? "me" : agent} /><div className="message-content"><div className="message-meta"><strong>{message.role === "user" ? "我" : agent === "deepseek" ? "DeepSeek" : "Codex"}</strong>{message.role !== "user" && <span className="agent-label">你的 Agent</span>}<time>{clock(message.created_at)}</time><div className="message-actions"><button onClick={() => quote(message)} aria-label={`回复${message.role === "user" ? "我的" : `${agent === "deepseek" ? "DeepSeek" : "Codex"} 的`}消息`} title="回复"><Icon name="reply" size={14} /></button><button onClick={() => copy(message.text)} aria-label="复制消息" title="复制"><Icon name="copy" size={14} /></button></div></div><div className="message-bubble"><MessageBody text={message.text} /></div></div></article>)}
        {running(snapshot.session?.status) && <div className="working-message" role="status"><Avatar agent={agent} small /><span className="working-dots"><i /><i /><i /></span><span>{statusLabels[snapshot.session?.status ?? "running"]}</span></div>}
      </div>
      <div className="composer-area">
        {snapshot.approvals.map(approval => <div className="approval-card" key={approval.id}><div><Icon name="shield" size={18} /><strong>Codex 需要你批准{approval.method.includes("fileChange") ? "文件修改" : "命令执行"}</strong></div><pre>{approval.command ?? "请检查执行记录中的文件变更。"}</pre>{approval.cwd && <small>{approval.cwd}</small>}<p>{approval.reason}</p><div className="approval-actions"><button className="outline-button" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: false })}>拒绝</button><button className="primary" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: true })}>批准本次</button></div></div>)}
        {(error || snapshot.session?.error) && <div className="inline-error" role="alert">{error ?? snapshot.session?.error}</div>}{probe?.sidebar_warning && <div className="sync-notice">最近列表同步未完成，私聊仍可使用。{probe.sidebar_warning}</div>}
        {agent === "deepseek" && <div className="sync-notice">DeepSeek 按消息显示回复。终端执行暂不可用，文件工具可用。</div>}
        <form className="composer" onSubmit={event => { event.preventDefault(); void send(); }}>
          {reply && <div className="reply-preview"><Icon name="reply" size={16} /><div><strong>回复 {reply.author}</strong><p>{reply.text.slice(0, 140)}</p></div><button type="button" className="icon-button" aria-label="取消回复" onClick={() => setReply(null)}><Icon name="close" size={15} /></button></div>}
          {!!files.length && <div className="attachment-list">{files.map((file, index) => <div className="attachment-chip" key={`${file.name}-${index}`}><Icon name="file" size={18} /><span>{file.name}<small>{(file.size / 1024).toFixed(1)} KB · 文本</small></span><button type="button" aria-label={`移除附件 ${file.name}`} onClick={() => setFiles(current => current.filter((_, i) => i !== index))}><Icon name="close" size={13} /></button></div>)}</div>}
          <label className="sr-only" htmlFor="chat-input">输入消息</label><textarea id="chat-input" ref={input} value={text} onChange={event => setText(event.target.value)} placeholder={group ? "说说你的想法，输入 @ 指定一位成员…" : "把想法写下来，我们一起往前推进…"} disabled={busy} maxLength={25000} rows={3} onKeyDown={event => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(); } }} />
          <div className="composer-toolbar"><div className="composer-tools"><input className="sr-only" type="file" ref={fileInput} multiple accept=".txt,.md,.json,.csv,.ts,.tsx,.js,.jsx,.py,.rs,.html,.css,.yaml,.yml,.log" onChange={attach} tabIndex={-1} aria-label="选择文本附件" /><button className="icon-button" type="button" title="添加文本附件，合计不超过 60 KB" aria-label="添加文本附件" disabled={busy} onClick={() => fileInput.current?.click()}><Icon name="attach" size={20} /></button><div className="mention-control"><button className={`icon-button ${mentions ? "selected" : ""}`} type="button" aria-label="指定 Agent" disabled={busy} onClick={() => setMentions(!mentions)}><Icon name="at" size={19} /></button>{mentions && <div className="mention-menu"><div className="menu-caption">发送给哪位成员</div>{members.map(id => <button type="button" key={id} onClick={() => { setTarget(id); setText(current => `@${id === "codex" ? "Codex" : "DeepSeek"} ${current.replace(/^@\w+\s*/, "")}`); setMentions(false); input.current?.focus(); }}><Avatar agent={id} small /><span>{id === "codex" ? "Codex" : "DeepSeek"}<small>{id === "codex" ? "可执行任务" : group ? "请使用独立私聊" : "可执行任务"}</small></span>{target === id && <Icon name="check" size={16} />}</button>)}</div>}</div><span className="toolbar-divider" /><label className="model-picker"><span className="sr-only">选择模型</span><select aria-label="选择模型" value={model} onChange={event => onModel(event.target.value)} disabled={!probe || busy}>{!probe && <option value="">{connecting ? "正在连接…" : "选择模型"}</option>}{probe?.models.map(m => <option key={m.model} value={m.model}>{m.displayName}</option>)}</select><Icon name="down" size={13} /></label></div><div className="composer-submit">{!probe && <button type="button" className="connect-link" onClick={onConnect} disabled={connecting}>{connecting ? "连接中…" : `连接 ${agent === "deepseek" ? "DeepSeek" : "Codex"}`}</button>}{busy ? <button className="send-button stop-button" type="button" aria-label="取消任务" disabled={acting || ["starting", "cancel_requested", "stopping"].includes(snapshot.session?.status ?? "starting")} onClick={() => action(agent === "deepseek" ? "deepseek_cancel" : "codex_cancel")}><Icon name="stop" size={18} /></button> : <button className="send-button" type="submit" aria-label="发送消息" title={!desktop ? "请在桌面版中发送" : "发送消息"} disabled={!ready || !text.trim() || !desktop}><Icon name="send" size={21} /></button>}</div></div>
        </form><div className="composer-footer"><span>{group ? `发送给 ${target === "codex" ? "Codex" : "DeepSeek（仅私聊）"}` : "私聊记录独立保存"}</span><span>Enter 发送<span className="keyboard-separator">·</span>Shift + Enter 换行</span></div>
      </div>
    </div>{panel && <aside className="detail-panel" aria-label={panel === "members" ? "聊天成员" : "执行记录"}><div className="panel-heading"><h2>{panel === "members" ? "聊天成员" : "执行记录"}</h2><button className="icon-button" aria-label="关闭详情" onClick={() => setPanel(null)}><Icon name="close" size={17} /></button></div>{panel === "members" ? <><p className="panel-caption">{members.length} 位 Agent · 1 位成员</p><div className="person-row"><Avatar agent="me" small /><div><strong>我</strong><small>群主 · 本机用户</small></div><span className="availability ready">你</span></div>{members.map(id => <div className="person-row" key={id}><Avatar agent={id} small /><div><strong>{id === "codex" ? "Codex" : "DeepSeek"}</strong><small>{agents.find(a => a.id === id)?.description}</small></div><span className={`availability ${id === agent ? "ready" : ""}`}>{id === agent ? probe ? "已就绪" : "可连接" : "仅独立私聊"}</span></div>)}{group && <button className="outline-button panel-add" onClick={onInvite}><Icon name="plus" size={16} />管理本机 Agent</button>}<div className="panel-note"><Icon name="shield" /><p>这里只包含你本机的 Agent。私聊不会自动共享到群里。</p></div></> : <><div className="execution-status"><span className={`presence-dot ${busy ? "connected" : ""}`} /><strong>{statusLabels[snapshot.session?.status ?? "idle"] ?? "就绪"}</strong><small>{tools.length} 条工具记录</small></div>{tools.length ? tools.map(tool => <details className="panel-tool" key={tool.id}><summary><Icon name="terminal" size={15} />工具记录<Icon name="down" size={13} /></summary><pre>{tool.text}</pre></details>) : <div className="panel-empty"><Icon name="terminal" size={29} /><h3>工作开始后，在这里回看。</h3><p>命令、文件变更和执行结果会随会话保留。</p></div>}{project && <div className="project-context"><span>当前项目</span><strong>{project.name}</strong><code>{project.primary_root}</code></div>}</>}</aside>}</div>
  </section>;
}
