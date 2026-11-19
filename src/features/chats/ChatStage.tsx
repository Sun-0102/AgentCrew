import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/Icon";
import Avatar from "../../components/Avatar";
import MessageBody from "./MessageBody";
import { agents, clock, desktop, running, statusLabels, taskLabels, type Group, type Message, type Probe, type Scope, type Snapshot } from "./api";
import type { Project } from "../projects/api";

interface Props {
  scope: Scope; scopeKey: string; title: string; group?: Group; project?: Project; agent?: "codex" | "deepseek";
  snapshot: Snapshot; probe: Probe | null; model: string; connecting: boolean;
  onAgent: (agent: "codex" | "deepseek") => void;
  collaborationModels: { codex: string; deepseek: string };
  onModel: (model: string) => void; onConnect: () => void; onInvite: () => void; onSettings: () => void;
  onRefresh: () => Promise<void>; notify: (text: string) => void;
}
interface Attachment { name: string; text: string; size: number }
interface Reply { author: string; text: string }
const drafts = new Map<string, { text: string; files: Attachment[]; reply: Reply | null }>();

export default function ChatStage(p: Props) {
  const { scope, scopeKey, title, group, project, agent = "codex", snapshot, probe, model, connecting, onAgent, onModel, onConnect, onInvite, onSettings, onRefresh, notify } = p;
  const [text, setText] = useState(() => drafts.get(scopeKey)?.text ?? localStorage.getItem(`ai-gent.draft.${scopeKey}`) ?? "");
  const [files, setFiles] = useState<Attachment[]>(() => drafts.get(scopeKey)?.files ?? []);
  const [reply, setReply] = useState<Reply | null>(() => drafts.get(scopeKey)?.reply ?? null);
  const [sending, setSending] = useState(false);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<"members" | "evidence" | null>(null);
  const [mentions, setMentions] = useState(false);
  const task = snapshot.task;
  const taskActive = !!task && ["running", "cancelling"].includes(task.status);
  const taskControllable = !!task && ["running", "paused", "cancelling"].includes(task.status);
  const target = agent;
  const executingAgent = snapshot.agent ?? agent;
  const feed = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const nearBottom = useRef(true);
  const members = group?.members ?? [agent];
  const busy = sending || running(snapshot.session?.status) || taskActive;
  const composingBlocked = sending || (group ? task?.status === "cancelling" : busy);
  const namedTarget = text.trimStart().match(/^@(Codex|DeepSeek)\b/i)?.[1].toLowerCase();
  const recipient = namedTarget === "codex" || namedTarget === "deepseek" ? namedTarget : group && taskActive ? task!.current_agent : agent;
  const ready = group ? !!p.collaborationModels[recipient] && members.includes(recipient) : !!probe && (!probe.requires_auth || !!probe.auth) && !!model && members.includes(recipient);
  const visibleMessages: Message[] = [];
  let replyBlock: Message | null = null;
  for (const message of snapshot.messages) {
    if (message.role === "assistant") {
      if (!message.text.trim()) continue;
      const speaker = message.agent ?? agent;
      if (replyBlock?.agent === speaker) {
        replyBlock.text += `\n\n${message.text.trimEnd()}`;
      } else {
        replyBlock = { ...message, agent: speaker, text: message.text.trimEnd() };
        visibleMessages.push(replyBlock);
      }
    } else if (message.role !== "tool") {
      // New input, dispatch or workspace/status changes separate replies; tool activity does not.
      replyBlock = null;
      if (message.role === "user" && message.text.trim()) visibleMessages.push(message);
    }
  }
  const records = snapshot.messages.filter(m => m.role !== "user" && m.role !== "assistant");
  const hasMessages = visibleMessages.length > 0;
  const providerWorking = running(snapshot.session?.status);
  const attention = !!error || !!snapshot.session?.error || !!probe?.sidebar_warning || !!task && ["blocked", "interrupted"].includes(task.status);
  const stateText = error ? "操作未完成" : task?.status === "cancelling" ? "正在停止" : snapshot.approvals.length ? "Codex 等待你批准" : providerWorking ? `${executingAgent === "deepseek" ? "DeepSeek" : "Codex"} ${statusLabels[snapshot.session?.status ?? "running"] ?? "正在处理"}${task?.status === "paused" ? " · 结束后暂停" : ""}` : sending ? "正在发送" : task ? task.status === "running" ? `${task.current_agent === "deepseek" ? "DeepSeek" : "Codex"} 正在处理` : taskLabels[task.status] ?? task.status : snapshot.session ? statusLabels[snapshot.session.status] ?? "就绪" : connecting ? "正在连接" : ready ? "就绪" : "尚未连接";
  useEffect(() => {
    drafts.set(scopeKey, { text, files, reply });
    try { if (text) localStorage.setItem(`ai-gent.draft.${scopeKey}`, text); else localStorage.removeItem(`ai-gent.draft.${scopeKey}`); }
    catch { setError("草稿未能保存，请检查本机存储空间。"); }
  }, [scopeKey, text, files, reply]);
  useEffect(() => { if (feed.current && (!hasMessages || nearBottom.current)) feed.current.scrollTop = hasMessages ? feed.current.scrollHeight : 0; }, [hasMessages, snapshot.messages.map(m => m.text).join("")]);
  useEffect(() => {
    if (!group || composingBlocked) return;
    if (members.includes(recipient) && recipient !== agent) onAgent(recipient);
  }, [recipient, agent, members.join(","), composingBlocked]);
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
    if (!text.trim() || composingBlocked || !ready) return;
    if (!desktop) { notify("请在桌面版中发送消息，浏览器预览不会调用 Agent"); return; }
    const targetAgent = recipient;
    if (!members.includes(targetAgent)) { setError("这个 Agent 不在当前群里，请先邀请成员。"); return; }
    let content = text.trim();
    if (reply) content = `> 回复 ${reply.author}：${reply.text.replace(/\n/g, " ").slice(0, 500)}\n\n${content}`;
    for (const file of files) {
      const fence = "`".repeat((file.text.match(/`+/g) ?? []).reduce((size, run) => Math.max(size, run.length + 1), 3));
      content += `\n\n文本附件：${file.name}\n${fence}\n${file.text}\n${fence}`;
    }
    setSending(true); setError(null); nearBottom.current = true;
    try {
      if (group) {
        await invoke("group_task_start", { groupId: group.id, targetAgent, codexModel: p.collaborationModels.codex, deepseekModel: p.collaborationModels.deepseek, text: content });
      } else await invoke(agent === "deepseek" ? "deepseek_send" : "codex_send", { ...scope, model, text: content, targetAgent });
      setText(""); setFiles([]); setReply(null); await onRefresh();
    }
    catch (cause) { setError(String(cause)); } finally { setSending(false); }
  }
  async function action(command: string, args: Record<string, unknown> = {}) {
    setActing(true); setError(null);
    try { await invoke(command, { ...scope, ...args }); await onRefresh(); }
    catch (cause) { setError(String(cause)); } finally { setActing(false); }
  }
  function author(message: Message) { return message.role === "user" ? "我" : (message.agent ?? agent) === "deepseek" ? "DeepSeek" : "Codex"; }
  function quote(message: Message) { setReply({ author: author(message), text: message.text }); input.current?.focus(); }
  function editText(event: ChangeEvent<HTMLTextAreaElement>) {
    const field = event.currentTarget;
    setText(field.value);
    setMentions(/^\s*@[a-z]*$/i.test(field.value.slice(0, field.selectionStart)));
  }
  function chooseMention(id: string) {
    const name = id === "deepseek" ? "DeepSeek" : "Codex";
    onAgent(id === "deepseek" ? "deepseek" : "codex");
    setText(current => `@${name} ${current.replace(/^\s*@\w*\s*/, "")}`);
    setMentions(false);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(name.length + 2, name.length + 2);
    });
  }
  return <section className="chat-stage" aria-label={title}>
    <header className="chat-header"><div className="chat-title"><Avatar agent={group ? "group" : agent} /><div><h1 title={title}>{title}</h1><p><span className={`presence-dot ${ready ? "connected" : ""}`} />{group ? project ? `关联项目 · ${project.name}` : "本机协作群 · 独立上下文" : project ? project.name : "独立私聊 · 无需选择项目"}</p></div></div><div className="chat-header-actions"><button className={`members-trigger ${panel === "members" ? "selected" : ""}`} onClick={() => setPanel(panel === "members" ? null : "members")} aria-label="查看聊天成员"><span className="stacked-avatars">{members.map(id => <Avatar agent={id} small key={id} />)}</span><span>{members.length} 位 Agent<span className="members-human"> · 你</span></span><Icon name="down" size={14} /></button>{group && <button className="outline-button invite-button" onClick={onInvite}><Icon name="settings" size={17} />群设置</button>}<button className={`icon-button ${panel === "evidence" ? "selected" : ""}`} onClick={() => setPanel(panel === "evidence" ? null : "evidence")} aria-label="查看聊天详情" title="聊天详情"><Icon name="panel" /></button><button className="icon-button" aria-label="连接与设置" onClick={onSettings}><Icon name="more" /></button></div></header>
    <div className={`chat-stage-body ${panel ? "with-panel" : ""}`}><div className="chat-main">
      <div className="message-feed" ref={feed} onScroll={event => { const el = event.currentTarget; nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }} role="log" aria-label="聊天消息">
        {!hasMessages && <div className="conversation-welcome"><div className={`welcome-mark ${group ? "group-mark" : ""}`}>{group ? <Icon name="group" size={37} /> : <Icon name="spark" size={37} />}</div><h2>{group ? "围绕一个目标，一起讨论。" : "今天，想一起完成什么？"}</h2><p>{group ? "说说你的目标，或 @ 一位成员开始。群聊与私聊分别保留记录。" : `讨论一个想法，写一段代码，或把手头的事情交给 ${agent === "deepseek" ? "DeepSeek" : "Codex"}。`}</p><div className="prompt-suggestions">{(group ? ["一起梳理这个需求", "帮我检查一下方案"] : ["帮我理清一个想法", "一起开始一个新任务", "帮我检查一段代码"]).map(prompt => <button key={prompt} onClick={() => { setText(prompt); input.current?.focus(); }}>{prompt}<Icon name="right" size={14} /></button>)}</div></div>}
        {visibleMessages.map(message => <article className={`message-row ${message.role === "user" ? "outgoing" : "incoming"}`} key={message.id}><Avatar agent={message.role === "user" ? "me" : message.agent ?? agent} /><div className="message-content"><div className="message-meta"><strong>{author(message)}</strong><time>{clock(message.created_at)}</time><div className="message-actions"><button onClick={() => quote(message)} aria-label={`回复${message.role === "user" ? "我的" : `${author(message)} 的`}消息`} title="回复"><Icon name="reply" size={14} /></button><button onClick={() => copy(message.text)} aria-label="复制消息" title="复制"><Icon name="copy" size={14} /></button></div></div><div className="message-bubble"><MessageBody text={message.text.trimEnd()} /></div></div></article>)}
      </div>
      <div className="composer-area">
        <div className={`chat-status ${attention ? "needs-attention" : ""}`} role="status" aria-live="polite">
          <span className={`presence-dot ${busy ? "connected" : ""}`} /><span className="chat-status-text">{stateText}{task && task.queued_count > 0 ? ` · ${task.queued_count} 项待处理` : ""}</span>
          {(attention || snapshot.approvals.length > 0) && <button type="button" className="status-detail-button" onClick={() => setPanel("evidence")}>{snapshot.approvals.length ? "查看请求" : "查看详情"}</button>}
        </div>
        <form className="composer" onSubmit={event => { event.preventDefault(); void send(); }}>
          {reply && <div className="reply-preview"><Icon name="reply" size={16} /><div><strong>回复 {reply.author}</strong><p>{reply.text.slice(0, 140)}</p></div><button type="button" className="icon-button" aria-label="取消回复" onClick={() => setReply(null)}><Icon name="close" size={15} /></button></div>}
          {!!files.length && <div className="attachment-list">{files.map((file, index) => <div className="attachment-chip" key={`${file.name}-${index}`}><Icon name="file" size={18} /><span>{file.name}<small>{(file.size / 1024).toFixed(1)} KB · 文本</small></span><button type="button" aria-label={`移除附件 ${file.name}`} onClick={() => setFiles(current => current.filter((_, i) => i !== index))}><Icon name="close" size={13} /></button></div>)}</div>}
          <label className="sr-only" htmlFor="chat-input">输入消息</label><textarea id="chat-input" ref={input} value={text} onChange={editText} placeholder={group ? "说说你的想法，输入 @ 指定一位成员…" : "把想法写下来，我们一起往前推进…"} disabled={composingBlocked} maxLength={25000} rows={3} onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (mentions && event.key === "Escape") { event.preventDefault(); setMentions(false); return; }
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              if (mentions) {
                const query = text.slice(0, event.currentTarget.selectionStart).match(/^\s*@([a-z]*)$/i)?.[1].toLowerCase() ?? "";
                chooseMention(members.find(id => query ? id.startsWith(query) : id === target) ?? members[0]);
              } else void send();
            }
          }} />
          <div className="composer-toolbar"><div className="composer-tools"><input className="sr-only" type="file" ref={fileInput} multiple accept=".txt,.md,.json,.csv,.ts,.tsx,.js,.jsx,.py,.rs,.html,.css,.yaml,.yml,.log" onChange={attach} tabIndex={-1} aria-label="选择文本附件" /><button className="icon-button" type="button" title="添加文本附件，合计不超过 60 KB" aria-label="添加文本附件" disabled={composingBlocked} onClick={() => fileInput.current?.click()}><Icon name="attach" size={20} /></button><div className="mention-control"><button className={`icon-button ${mentions ? "selected" : ""}`} type="button" aria-label="指定 Agent" disabled={composingBlocked} onClick={() => setMentions(!mentions)}><Icon name="at" size={19} /></button>{mentions && <div className="mention-menu"><div className="menu-caption">发送给哪位成员</div>{members.map(id => <button type="button" key={id} disabled={composingBlocked} onClick={() => chooseMention(id)}><Avatar agent={id} small /><span>{id === "codex" ? "Codex" : "DeepSeek"}<small>{id === "codex" ? "可执行任务" : "可执行文件任务"}</small></span>{target === id && <Icon name="check" size={16} />}</button>)}</div>}</div><span className="toolbar-divider" /><label className="model-picker"><span className="sr-only">选择模型</span><select aria-label="选择模型" value={model} onChange={event => onModel(event.target.value)} disabled={!probe || composingBlocked}>{!probe && <option value="">{connecting ? "正在连接…" : "选择模型"}</option>}{probe?.models.map(m => <option key={m.model} value={m.model}>{m.displayName}</option>)}</select><Icon name="down" size={13} /></label></div><div className="composer-submit">{!probe && <button type="button" className="connect-link" onClick={onConnect} disabled={connecting}>{connecting ? "连接中…" : `连接 ${agent === "deepseek" ? "DeepSeek" : "Codex"}`}</button>}{(busy || taskControllable) && <button className="send-button stop-button" type="button" aria-label="停止任务" disabled={acting || sending || (taskControllable ? task?.status === "cancelling" : ["starting", "cancel_requested", "stopping"].includes(snapshot.session?.status ?? "starting"))} onClick={() => taskControllable && task ? action("group_task_control", { taskId: task.id, action: "cancel" }) : action(executingAgent === "deepseek" ? "deepseek_cancel" : "codex_cancel")}><Icon name="stop" size={18} /></button>}{(group || !busy) && <button className="send-button" type="submit" aria-label="发送消息" title={!desktop ? "请在桌面版中发送" : group && busy ? "发送插话，本轮结束后处理" : "发送消息"} disabled={composingBlocked || !ready || !text.trim() || !desktop}><Icon name="send" size={21} /></button>}</div></div>
        </form><div className="composer-footer"><span>{group ? `发送给 ${recipient === "codex" ? "Codex" : "DeepSeek"}` : "私聊记录独立保存"}</span><span>Enter 发送<span className="keyboard-separator">·</span>Shift + Enter 换行</span></div>
      </div>
    </div>{panel && <aside className="detail-panel" aria-label={panel === "members" ? "聊天成员" : "聊天详情"}>
      <div className="panel-heading"><h2>{panel === "members" ? "聊天成员" : "聊天详情"}</h2><button className="icon-button" aria-label="关闭详情" onClick={() => setPanel(null)}><Icon name="close" size={17} /></button></div>
      {panel === "members" ? <><p className="panel-caption">{members.length} 位 Agent · 1 位成员</p><div className="person-row"><Avatar agent="me" small /><div><strong>我</strong><small>群主 · 本机用户</small></div><span className="availability ready">你</span></div>{members.map(id => <div className="person-row" key={id}><Avatar agent={id} small /><div><strong>{id === "codex" ? "Codex" : "DeepSeek"}</strong><small>{agents.find(a => a.id === id)?.description}</small></div><span className={`availability ${id === agent ? "ready" : ""}`}>{id === agent ? probe ? "已就绪" : "可连接" : "可指定执行"}</span></div>)}{group && <button className="outline-button panel-add" onClick={onInvite}><Icon name="settings" size={16} />群设置与项目</button>}<div className="panel-note"><Icon name="shield" /><p>这里只包含你本机的 Agent。私聊不会自动共享到群里。</p></div></> : <>
        {group && task && <div className="collaboration-card" aria-label="协作任务">
          <div className="collaboration-heading"><strong>{taskLabels[task.status] ?? task.status}</strong><span>{task.current_agent === "codex" ? "Codex" : "DeepSeek"} · 已执行 {task.turn_count} 次{task.queued_count > 0 ? ` · ${task.queued_count} 条待处理` : ""}</span></div>
          <p className="collaboration-objective" title={task.objective}>{task.objective}</p>
          <details><summary>任务与模型</summary><p>任务：{task.id}</p><p>Codex：{task.codex_model}<br />DeepSeek：{task.deepseek_model}</p><p>交接内容在下方“交接详情”记录中查看。本轮回复不代表已验证通过，完成与否由你确认。</p></details>
          {task.error && <p className="collaboration-reason">{task.error}</p>}
          <div className="collaboration-actions">
            {task.status === "running" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "pause" })}>暂停接力</button>}
            {task.status === "paused" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "resume" })}>继续接力</button>}
            {["running", "paused", "cancelling"].includes(task.status) && <button type="button" className="outline-button" disabled={acting || task.status === "cancelling"} onClick={() => action("group_task_control", { taskId: task.id, action: "cancel" })}>停止</button>}
            {["idle", "awaiting_review"].includes(task.status) && <button type="button" className="primary" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "accept" })}>确认完成</button>}
          </div>
        </div>}
        {snapshot.approvals.map(approval => <div className="approval-card" key={approval.id}><div><Icon name="shield" size={18} /><strong>Codex 需要你批准{approval.method.includes("fileChange") ? "文件修改" : "命令执行"}</strong></div><pre>{approval.command ?? "请检查执行记录中的文件变更。"}</pre>{approval.cwd && <small>{approval.cwd}</small>}<p>{approval.reason}</p><div className="approval-actions"><button className="outline-button" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: false })}>拒绝</button><button className="primary" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: true })}>批准本次</button></div></div>)}
        {(error || snapshot.session?.error) && <div className="inline-error" role="alert">{error ?? snapshot.session?.error}</div>}
        {probe?.sidebar_warning && <div className="sync-notice">最近列表同步未完成，私聊仍可使用。{probe.sidebar_warning}</div>}
        {agent === "deepseek" && <p className="panel-caption">DeepSeek 按消息更新，文件工具可用，终端暂不可用。</p>}
        <div className="execution-status"><strong>执行详情</strong><small>{records.length} 条记录</small></div>
        {records.length ? records.map(record => <details className="panel-tool" key={record.id}>
          <summary><Icon name={record.role === "workspace" ? "folder" : record.role === "assignment" ? "at" : "terminal"} size={15} /><span>{record.role === "workspace" ? "工作目录变更" : record.role === "assignment" ? `${author(record)} · 交接详情` : record.role === "system" ? "状态记录" : `${author(record)} · 工具记录`}</span><Icon name="down" size={13} /></summary><pre>{record.text}</pre>
        </details>) : <p className="panel-caption">暂无执行记录。</p>}
        {(project || snapshot.session?.workspace_path) && <div className="project-context"><span>{project ? "当前项目" : "本群工作目录"}</span>{project && <strong>{project.name}</strong>}<code>{project?.primary_root ?? snapshot.session?.workspace_path}</code></div>}
      </>}
    </aside>}</div>
  </section>;
}
