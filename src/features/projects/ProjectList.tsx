import { useState } from "react";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { openProjectDirectory, removeProject, type Project } from "./api";

export default function ProjectList({ projects, selectedId, onSelect, onRemoved, onError }: {
  projects: Project[];
  selectedId?: string;
  onSelect: (id: string) => void;
  onRemoved: (project: Project) => void;
  onError: (message: string) => void;
}) {
  const [target, setTarget] = useState<Project | null>(null);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");

  async function confirmRemove() {
    if (!target || removing) return;
    setRemoving(true);
    setError("");
    try {
      await removeProject(target.id);
      onRemoved(target);
      setTarget(null);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setRemoving(false);
    }
  }

  return <>
    {projects.map(project => <div className={`conversation-item project-item ${selectedId === project.id ? "selected" : ""}`} key={project.id}>
      <button type="button" className="folder-avatar project-folder" aria-label={`选择项目 ${project.name}`} aria-pressed={selectedId === project.id} onClick={() => onSelect(project.id)}><Icon name="folder" size={21} /></button>
      <button type="button" className="project-name" title={project.name} aria-pressed={selectedId === project.id} onClick={() => onSelect(project.id)}>{project.name}</button>
      <button type="button" className="project-path" title={`打开本地文件夹：${project.primary_root}`} aria-label={`打开本地文件夹：${project.primary_root}`} onClick={() => { void openProjectDirectory(project.id).catch(cause => onError(String(cause))); }}><span>{project.primary_root}</span><Icon name="external" size={12} /></button>
      <button type="button" className="icon-button project-remove" title="移除项目" aria-label={`移除项目 ${project.name}`} onClick={() => { setError(""); setTarget(project); }}><Icon name="trash" size={16} /></button>
    </div>)}
    {target && <Dialog title="移除项目" onClose={() => { if (!removing) setTarget(null); }}>
      <p className="session-dialog-intro">确定从工作空间移除「{target.name}」吗？</p>
      <p className="delete-session-description">本地文件、会话记录和关联群聊均保留。重新添加同一目录可恢复此项目。</p>
      {error && <p className="inline-error" role="alert">{error}</p>}
      <div className="dialog-footer"><span /><div className="session-dialog-actions"><button type="button" className="outline-button" disabled={removing} onClick={() => setTarget(null)}>取消</button><button type="button" className="primary danger-button" disabled={removing} onClick={() => void confirmRemove()}>{removing ? "正在移除…" : "确认移除"}</button></div></div>
    </Dialog>}
  </>;
}
