import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/Icon";
import Avatar from "../../components/Avatar";
import MessageBody from "./MessageBody";
import TaskResults from "./TaskResults";
import TaskActivity from "./TaskActivity";
import { replyBlocks } from "./activity";
import { speedChoices } from "./speed";
import ReasoningPicker from "./ReasoningPicker";
import { activeMention, insertMention, messageRecipients } from "./mentions";
import GroupMemberStatus from "./GroupMemberStatus";
import { skillPrefix, requestedSkills, type Skill } from "./skills";
import { agents, clock, desktop, running, statusLabels, taskLabels, type Group, type Message, type Probe, type Scope, type Snapshot } from "./api";
import type { Project } from "../projects/api";

interface Props {
  scope: Scope; scopeKey: string; title: string; group?: Group; project?: Project; agent?: "codex" | "deepseek";
  snapshot: Snapshot; probe: Probe | null; model: string; connecting: boolean;
  onAgent: (agent: "codex" | "deepseek") => void;
  collaborationModels: { codex: string; deepseek: string };
  codexServiceTier?: string; onServiceTier?: (value: string) => void;
  reasoningEffort?: string; collaborationEfforts?: { codex: string; deepseek: string };
  onReasoningEffort?: (value: string) => void;
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
  const [resultsOpen, setResultsOpen] = useState(false);
  const [mentions, setMentions] = useState(false);
  const [skillQuery, setSkillQuery] = useState<string | null>(null);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [skillIndex, setSkillIndex] = useState(0);
  const skillItems = skills.filter(skill => !skillQuery || `${skill.name} ${skill.description} ${skill.source}`.toLowerCase().includes(skillQuery.toLowerCase()));
  const fastSpeed = speedChoices(probe?.models.find(m => m.model === model)).find(t => t.id === "priority" || t.id === "fast");
  const fastMode = !!p.codexServiceTier && p.codexServiceTier !== "default";
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
  const busy = sending || running(snapshot.session?.status) || Object.values(snapshot.sessions ?? {}).some(session => running(session?.status)) || taskActive;
  const composingBlocked = sending || (group ? task?.status === "cancelling" : busy);
  const recipients = group ? messageRecipients(text, members) : [agent];
  const namedTarget = recipients.length === 1 ? recipients[0] : agent;
  const recipient = namedTarget === "deepseek" ? "deepseek" : "codex";
  const ready = group ? recipients.every(id => members.includes(id) && !!p.collaborationModels[id as "codex" | "deepseek"]) : !!probe && (!probe.requires_auth || !!probe.auth) && !!model;
  const visibleMessages = replyBlocks(snapshot, agent);
  const records = snapshot.messages.filter(m => m.role !== "user" && m.role !== "assistant");
  const hasMessages = visibleMessages.length > 0;
  const providerWorking = running(snapshot.session?.status);
  const attention = !!task?.members?.some(member => member.error) || !!error || !!snapshot.session?.error || !!probe?.sidebar_warning || !!task && ["blocked", "interrupted"].includes(task.status);
  const stateText = error ? "操作未完成" : task?.status === "cancelling" ? "正在停止" : snapshot.approvals.length ? "Codex 等待你批准" : providerWorking ? `${executingAgent === "deepseek" ? "DeepSeek" : "Codex"} ${statusLabels[snapshot.session?.status ?? "running"] ?? "正在处理"}${task?.status === "paused" ? " · 结束后暂停" : ""}` : sending ? "正在发送" : task ? task.status === "running" ? `${task.current_agent === "deepseek" ? "DeepSeek" : "Codex"} 正在处理` : taskLabels[task.status] ?? task.status : snapshot.session ? statusLabels[snapshot.session.status] ?? "就绪" : connecting ? "正在连接" : ready ? "就绪" : "尚未连接";
  useEffect(() => {
    drafts.set(scopeKey, { text, files, reply });
    try { if (text) localStorage.setItem(`ai-gent.draft.${scopeKey}`, text); else localStorage.removeItem(`ai-gent.draft.${scopeKey}`); }
    catch { setError("草稿未能保存，请检查本机存储空间。"); }
  }, [scopeKey, text, files, reply]);
  useEffect(() => { if (feed.current && (!hasMessages || nearBottom.current)) feed.current.scrollTop = hasMessages ? feed.current.scrollHeight : 0; }, [hasMessages, snapshot.messages.map(m => `${m.id}:${m.text.length}`).join("|")]);
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
  const skillsOpen = skillQuery !== null;
  useEffect(() => {
    if (skillsOpen) document.getElementById(`skill-option-${Math.min(skillIndex, skillItems.length - 1)}`)?.scrollIntoView({ block: "nearest" });
  }, [skillsOpen, skillIndex, skillItems.length]);
  useEffect(() => {
    if (!skillsOpen) return;
    let disposed = false;
    setSkills([]); setSkillsError(null); setSkillsLoading(true); setSkillIndex(0);
    if (!desktop) { setSkillsLoading(false); return; }
    invoke<Skill[]>("skills_list", { ...scope, agent }).then(items => {
      if (!disposed) setSkills(items);
    }).catch(cause => { if (!disposed) setSkillsError(String(cause)); })
      .finally(() => { if (!disposed) setSkillsLoading(false); });
    return () => { disposed = true; };
  }, [scopeKey, agent, skillsOpen]);
  function updateCommands(value: string, cursor: number) {
    setMentions(!!activeMention(value, cursor));
    setSkillQuery(skillPrefix(value.slice(0, cursor))?.[1] ?? null);
    setSkillIndex(0);
  }
  function chooseSkill(skill: Skill) {
    const cursor = input.current?.selectionStart ?? text.length;
    const prefix = text.slice(0, cursor);
    const query = skillPrefix(prefix)?.[1];
    if (query === undefined) return;
    const next = prefix.slice(0, prefix.length - query.length) + skill.name + " ";
    setText(next + text.slice(input.current?.selectionEnd ?? cursor));
    setSkillQuery(null);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(next.length, next.length); });
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
      const names = requestedSkills(text);
      if (names.length) content += await invoke<string>("skills_prepare", { ...scope, agents: recipients, names });
      if (group) {
        await invoke("group_task_start", { groupId: group.id, targetAgent, codexServiceTier: p.collaborationModels.codex ? p.codexServiceTier ?? "default" : null, codexModel: p.collaborationModels.codex, deepseekModel: p.collaborationModels.deepseek, codexReasoningEffort: (p.collaborationModels.codex && p.collaborationEfforts?.codex) || null, deepseekReasoningEffort: (p.collaborationModels.deepseek && p.collaborationEfforts?.deepseek) || null, text: content });
      } else await invoke(agent === "deepseek" ? "deepseek_send" : "codex_send", { ...scope, model, ...(agent === "codex" ? { serviceTier: p.codexServiceTier ?? "default" } : {}), reasoningEffort: p.reasoningEffort || null, text: content, targetAgent });
      setText(""); setFiles([]); setReply(null); setSkillQuery(null); await onRefresh();
    }
    catch (cause) { setError(String(cause)); } finally { setSending(false); }
  }
  async function action(command: string, args: Record<string, unknown> = {}, reportFailure = false) {
    setActing(true); setError(null);
    try { await invoke(command, { ...scope, ...args }); await onRefresh(); }
    catch (cause) { setError(String(cause)); if (reportFailure) throw cause; } finally { setActing(false); }
  }
  function author(message: Message) { return message.role === "user" ? "我" : (message.agent ?? agent) === "deepseek" ? "DeepSeek" : "Codex"; }
  function quote(message: Message) { setReply({ author: author(message), text: message.text }); input.current?.focus(); }
  function editText(event: ChangeEvent<HTMLTextAreaElement>) {
    const field = event.currentTarget;
    setText(field.value);
    updateCommands(field.value, field.selectionStart);
  }
  function chooseMention(id: string) {
    const name = id === "deepseek" ? "DeepSeek" : "Codex";
    const field = input.current;
    const next = insertMention(text, field?.selectionStart ?? text.length, field?.selectionEnd ?? text.length, name);
    setText(next.text);
    setMentions(false);
    setSkillQuery(null);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(next.cursor, next.cursor);
    });
  }
  return <section className="chat-stage" aria-label={title}>
    {resultsOpen && group && <TaskResults groupId={group.id} latestTask={task} acting={acting} members={members} onClose={() => setResultsOpen(false)} notify={notify} onAction={async value => { if (!task) throw new Error("任务不存在，请刷新后重试。"); await action("group_task_control", { taskId: task.id, action: value }, true); }} />}
    <header className="chat-header"><div className="chat-title"><Avatar agent={group ? "group" : agent} /><div><h1 title={title}>{title}</h1><p><span className={`presence-dot ${ready ? "connected" : ""}`} />{group ? project ? `关联项目 · ${project.name}` : "本机协作群 · 独立上下文" : project ? project.name : "独立私聊 · 无需选择项目"}</p></div></div><div className="chat-header-actions"><button className={`members-trigger ${panel === "members" ? "selected" : ""}`} onClick={() => setPanel(panel === "members" ? null : "members")} aria-label="查看聊天成员"><span className="stacked-avatars">{members.map(id => <Avatar agent={id} small key={id} />)}</span><span>{members.length} 位 Agent<span className="members-human"> · 你</span></span><Icon name="down" size={14} /></button>{group && <button type="button" className="outline-button task-results-trigger" onClick={() => setResultsOpen(true)}><Icon name="file" size={17} />任务结果</button>}{group && <button className="outline-button invite-button" onClick={onInvite}><Icon name="settings" size={17} />群设置</button>}<button className={`icon-button ${panel === "evidence" ? "selected" : ""}`} onClick={() => setPanel(panel === "evidence" ? null : "evidence")} aria-label="查看聊天详情" title="聊天详情"><Icon name="panel" /></button><button className="icon-button" aria-label="连接与设置" onClick={onSettings}><Icon name="more" /></button></div></header>
    <div className={`chat-stage-body ${panel ? "with-panel" : ""}`}><div className="chat-main">
      <div className="message-feed" ref={feed} onScroll={event => { const el = event.currentTarget; nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }} role="log" aria-label="聊天消息">
        {!hasMessages && <div className="conversation-welcome"><div className={`welcome-mark ${group ? "group-mark" : ""}`}>{group ? <Icon name="group" size={37} /> : <Icon name="spark" size={37} />}</div><h2>{group ? "围绕一个目标，一起讨论。" : "今天，想一起完成什么？"}</h2><p>{group ? "直接发消息，成员会一起协调分工；@ 谁就只交给谁。" : `讨论一个想法，写一段代码，或把手头的事情交给 ${agent === "deepseek" ? "DeepSeek" : "Codex"}。`}</p><div className="prompt-suggestions">{(group ? ["一起梳理这个需求", "帮我检查一下方案"] : ["帮我理清一个想法", "一起开始一个新任务", "帮我检查一段代码"]).map(prompt => <button key={prompt} onClick={() => { setText(prompt); input.current?.focus(); }}>{prompt}<Icon name="right" size={14} /></button>)}</div></div>}
        {visibleMessages.map(message => <article className={`message-row ${message.role === "user" ? "outgoing" : "incoming"}`} key={message.id}><Avatar agent={message.role === "user" ? "me" : message.agent ?? agent} /><div className="message-content"><div className="message-meta"><strong>{author(message)}</strong><time>{clock(message.created_at)}</time><div className="message-actions"><button disabled={!message.text} onClick={() => quote(message)} aria-label={`回复${message.role === "user" ? "我的" : `${author(message)} 的`}消息`} title="回复"><Icon name="reply" size={14} /></button><button disabled={!message.text} onClick={() => copy(message.text)} aria-label="复制消息" title="复制"><Icon name="copy" size={14} /></button></div></div>{message.role === "assistant" && <TaskActivity block={message} />}{message.text && <div className="message-bubble"><MessageBody text={message.text.trimEnd()} /></div>}</div></article>)}
      </div>
      <div className="composer-area">
        <div className={`chat-status ${attention ? "needs-attention" : ""}`} role="status" aria-live="polite">
          <span className={`presence-dot ${busy ? "connected" : ""}`} /><span className="chat-status-text">{group && task?.members?.length && task.status === "running" && !error && !snapshot.approvals.length ? "群任务正在处理" : stateText}{task && task.queued_count > 0 ? ` · ${task.queued_count} 项待处理` : ""}</span>
          {(attention || snapshot.approvals.length > 0) && <button type="button" className="status-detail-button" onClick={() => setPanel("evidence")}>{snapshot.approvals.length ? "查看请求" : "查看详情"}</button>}
        </div>
        {group && <GroupMemberStatus snapshot={snapshot} members={members} />}
        <form className="composer" onSubmit={event => { event.preventDefault(); void send(); }}>
          {reply && <div className="reply-preview"><Icon name="reply" size={16} /><div><strong>回复 {reply.author}</strong><p>{reply.text.slice(0, 140)}</p></div><button type="button" className="icon-button" aria-label="取消回复" onClick={() => setReply(null)}><Icon name="close" size={15} /></button></div>}
          {!!files.length && <div className="attachment-list">{files.map((file, index) => <div className="attachment-chip" key={`${file.name}-${index}`}><Icon name="file" size={18} /><span>{file.name}<small>{(file.size / 1024).toFixed(1)} KB · 文本</small></span><button type="button" aria-label={`移除附件 ${file.name}`} onClick={() => setFiles(current => current.filter((_, i) => i !== index))}><Icon name="close" size={13} /></button></div>)}</div>}
          {skillsOpen && <div className="skill-suggestions" id="skill-suggestions" role="listbox" aria-label="技能列表">
            <div className="menu-caption">{agent === "codex" ? "Codex" : "DeepSeek"} · 技能 {skillsLoading ? "" : `(${skills.length})`}</div>
            {skillsLoading ? <p role="status">正在读取本地和运行时技能…</p> : skillsError ? <p role="alert">{skillsError}</p> : !skillItems.length ? <p role="status">{desktop ? "没有匹配的技能。请检查搜索词或当前 Agent 的技能配置。" : "请在桌面版中读取本地技能。"}</p> : skillItems.map((skill, index) => <button type="button" role="option" id={`skill-option-${index}`} aria-selected={index === Math.min(skillIndex, skillItems.length - 1)} key={skill.name} disabled={composingBlocked} onMouseDown={event => event.preventDefault()} onClick={() => chooseSkill(skill)}><strong>/{skill.name}<small className="skill-source">{skill.source}</small></strong><span>{skill.description || "本地技能"}</span></button>)}
          </div>}
          <label className="sr-only" htmlFor="chat-input">输入消息</label><textarea id="chat-input" ref={input} value={text} onChange={editText} onSelect={event => updateCommands(event.currentTarget.value, event.currentTarget.selectionStart)} placeholder={group ? "输入 / 调用技能，或 @ 指定成员…" : "把想法写下来，输入 / 调用技能…"} aria-expanded={skillsOpen} aria-controls={skillsOpen ? "skill-suggestions" : undefined} aria-activedescendant={skillsOpen && skillItems.length ? `skill-option-${Math.min(skillIndex, skillItems.length - 1)}` : undefined} disabled={composingBlocked} maxLength={25000} rows={3} onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (skillsOpen) {
              if (event.key === "Escape") { event.preventDefault(); setSkillQuery(null); return; }
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                setSkillIndex(index => (index + (event.key === "ArrowDown" ? 1 : -1) + Math.max(1, skillItems.length)) % Math.max(1, skillItems.length));
                return;
              }
              if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
                if (skillItems.length) { event.preventDefault(); chooseSkill(skillItems[Math.min(skillIndex, skillItems.length - 1)]); return; }
                if (event.key === "Enter") { event.preventDefault(); return; }
              }
            }
            if (mentions && event.key === "Escape") { event.preventDefault(); setMentions(false); return; }
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              if (mentions) {
                const query = activeMention(text, event.currentTarget.selectionStart)?.query ?? "";
                chooseMention(members.find(id => query ? id.startsWith(query) : id === target) ?? members[0]);
              } else void send();
            }
          }} />
          <div className="composer-toolbar"><div className="composer-tools"><input className="sr-only" type="file" ref={fileInput} multiple accept=".txt,.md,.json,.csv,.ts,.tsx,.js,.jsx,.py,.rs,.html,.css,.yaml,.yml,.log" onChange={attach} tabIndex={-1} aria-label="选择文本附件" /><button className="icon-button" type="button" title="添加文本附件，合计不超过 60 KB" aria-label="添加文本附件" disabled={composingBlocked} onClick={() => fileInput.current?.click()}><Icon name="attach" size={20} /></button><div className="mention-control"><button className={`icon-button ${mentions ? "selected" : ""}`} type="button" aria-label="指定 Agent" disabled={composingBlocked} onClick={() => setMentions(!mentions)}><Icon name="at" size={19} /></button>{mentions && <div className="mention-menu"><div className="menu-caption">选择要 @ 的成员</div>{members.map(id => <button type="button" key={id} disabled={composingBlocked} onClick={() => chooseMention(id)}><Avatar agent={id} small /><span>{id === "codex" ? "Codex" : "DeepSeek"}<small>可执行任务</small></span>{target === id && <Icon name="check" size={16} />}</button>)}</div>}</div><span className="toolbar-divider" /><label className="model-picker"><span className="sr-only">选择模型</span><select aria-label="选择模型" value={model} onChange={event => onModel(event.target.value)} disabled={!probe || busy || taskControllable}>{!probe && <option value="">{connecting ? "正在连接…" : "选择模型"}</option>}{probe?.models.map(m => <option key={m.model} value={m.model}>{m.displayName}</option>)}</select><Icon name="down" size={13} /></label><ReasoningPicker key={`${agent}:${model}`} model={probe?.models.find(m => m.model === model)} value={p.reasoningEffort} disabled={busy || taskControllable || !probe} onChange={p.onReasoningEffort} />{agent === "codex" && <button type="button" className={`icon-button fast-mode-toggle ${fastMode ? "active" : ""}`} aria-label={fastMode ? "关闭快速模式" : "开启快速模式"} aria-pressed={fastMode} title={!fastSpeed && !fastMode ? "当前模型不支持快速模式" : fastMode ? "快速模式已开启，点击切回标准速度" : "开启快速模式，消耗更多额度"} disabled={!probe || (!fastSpeed && !fastMode) || busy || taskControllable} onClick={() => p.onServiceTier?.(fastMode ? "default" : fastSpeed!.id)}><Icon name="bolt" size={18} /></button>}</div><div className="composer-submit">{!probe && <button type="button" className="connect-link" onClick={onConnect} disabled={connecting}>{connecting ? "连接中…" : `连接 ${agent === "deepseek" ? "DeepSeek" : "Codex"}`}</button>}{(busy || taskControllable) && <button className="send-button stop-button" type="button" aria-label="停止任务" disabled={acting || sending || (taskControllable ? task?.status === "cancelling" : ["starting", "cancel_requested", "stopping"].includes(snapshot.session?.status ?? "starting"))} onClick={() => taskControllable && task ? action("group_task_control", { taskId: task.id, action: "cancel" }) : action(executingAgent === "deepseek" ? "deepseek_cancel" : "codex_cancel")}><Icon name="stop" size={18} /></button>}{(group || !busy) && <button className="send-button" type="submit" aria-label="发送消息" title={!desktop ? "请在桌面版中发送" : group && busy ? "发送插话，各成员在自己的当前轮结束后处理" : "发送消息"} disabled={composingBlocked || !ready || !text.trim() || !desktop}><Icon name="send" size={21} /></button>}</div></div>
        </form><div className="composer-footer"><span>{group ? `发送给 ${recipients.map(id => id === "codex" ? "Codex" : "DeepSeek").join("、")}${recipients.length > 1 ? " · 协调分工" : " · 单独处理"}` : "私聊记录独立保存"}</span><span>Enter 发送<span className="keyboard-separator">·</span>Shift + Enter 换行</span></div>
      </div>
    </div>{panel && <aside className="detail-panel" aria-label={panel === "members" ? "聊天成员" : "聊天详情"}>
      <div className="panel-heading"><h2>{panel === "members" ? "聊天成员" : "聊天详情"}</h2><button className="icon-button" aria-label="关闭详情" onClick={() => setPanel(null)}><Icon name="close" size={17} /></button></div>
      {panel === "members" ? <><p className="panel-caption">{members.length} 位 Agent · 1 位成员</p><div className="person-row"><Avatar agent="me" small /><div><strong>我</strong><small>群主 · 本机用户</small></div><span className="availability ready">你</span></div>{members.map(id => <div className="person-row" key={id}><Avatar agent={id} small /><div><strong>{id === "codex" ? "Codex" : "DeepSeek"}</strong><small>{agents.find(a => a.id === id)?.description}</small></div><span className={`availability ${id === agent ? "ready" : ""}`}>{id === agent ? probe ? "已就绪" : "可连接" : "可指定执行"}</span></div>)}{group && <button className="outline-button panel-add" onClick={onInvite}><Icon name="settings" size={16} />群设置与项目</button>}<div className="panel-note"><Icon name="shield" /><p>这里只包含你本机的 Agent。私聊不会自动共享到群里。</p></div></> : <>
        {group && task && <div className="collaboration-card" aria-label="协作任务">
          <div className="collaboration-heading"><strong>{taskLabels[task.status] ?? task.status}</strong><span>已执行 {task.turn_count} 次{task.queued_count > 0 ? ` · ${task.queued_count} 条待处理` : ""}</span></div>
          <p className="collaboration-objective" title={task.objective}>{task.objective}</p>
          <details><summary>任务与模型</summary><p>任务：{task.id}</p><p>Codex：{task.codex_model}<br />DeepSeek：{task.deepseek_model}</p><p>交接和文件依据可在“任务结果”中查看。本轮回复不代表已验证通过，完成与否由你确认。</p></details>
          {task.error && <p className="collaboration-reason">{task.error}</p>}
          <div className="collaboration-actions"><button type="button" className="outline-button" onClick={() => setResultsOpen(true)}>查看任务结果</button>
            {task.status === "running" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "pause" })}>暂停接力</button>}
            {task.status === "paused" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "resume" })}>继续接力</button>}
            {["running", "paused", "cancelling"].includes(task.status) && <button type="button" className="outline-button" disabled={acting || task.status === "cancelling"} onClick={() => action("group_task_control", { taskId: task.id, action: "cancel" })}>停止</button>}
            {["idle", "awaiting_review"].includes(task.status) && <button type="button" className="primary" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "accept" })}>确认完成</button>}
          </div>
        </div>}
        {snapshot.approvals.map(approval => <div className="approval-card" key={approval.id}><div><Icon name="shield" size={18} /><strong>Codex 需要你批准{approval.method.includes("fileChange") ? "文件修改" : "命令执行"}</strong></div><pre>{approval.command ?? "请检查执行记录中的文件变更。"}</pre>{approval.cwd && <small>{approval.cwd}</small>}<p>{approval.reason}</p><div className="approval-actions"><button className="outline-button" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: false })}>拒绝</button><button className="primary" disabled={acting} onClick={() => action("codex_approve", { id: approval.id, allow: true })}>批准本次</button></div></div>)}
        {(error || snapshot.session?.error) && <div className="inline-error" role="alert">{error ?? snapshot.session?.error}</div>}
        {probe?.sidebar_warning && <div className="sync-notice">最近列表同步未完成，私聊仍可使用。{probe.sidebar_warning}</div>}
        {agent === "deepseek" && <p className="panel-caption">DeepSeek 按消息更新，按当前运行时提供的工具执行任务。</p>}
        <div className="execution-status"><strong>执行详情</strong><small>{records.length} 条记录</small></div>
        {records.length ? records.map(record => <details className="panel-tool" key={record.id}>
          <summary><Icon name={record.role === "workspace" ? "folder" : record.role === "assignment" ? "at" : "terminal"} size={15} /><span>{record.role === "workspace" ? "工作目录变更" : record.role === "assignment" ? `${author(record)} · 交接详情` : record.role === "system" ? "状态记录" : `${author(record)} · 工具记录`}</span><Icon name="down" size={13} /></summary><pre>{record.text}</pre>
        </details>) : <p className="panel-caption">暂无执行记录。</p>}
        {(project || snapshot.session?.workspace_path) && <div className="project-context"><span>{project ? "当前项目" : "本群工作目录"}</span>{project && <strong>{project.name}</strong>}<code>{project?.primary_root ?? snapshot.session?.workspace_path}</code></div>}
      </>}
    </aside>}</div>
  </section>;
}
