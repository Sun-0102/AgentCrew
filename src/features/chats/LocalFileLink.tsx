import { useEffect, useId, useLayoutEffect, useRef, useState, type ComponentProps, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/Icon";
import { desktop } from "./api";

interface Props extends ComponentProps<"a"> {
  filePath: string;
  line?: number;
  workspacePath?: string;
  onOpenFile?: (path: string, line?: number) => void;
  onNotify?: (message: string) => void;
}

const revealLabel = /Mac|iPhone|iPad/.test(navigator.platform) ? "在 Finder 中显示" : "打开文件所在位置";

export default function LocalFileLink({ filePath, line, workspacePath, onOpenFile, onNotify, children, ...props }: Props) {
  const anchor = useRef<HTMLAnchorElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null);

  function close(restoreFocus = false) {
    panel.current?.hidePopover();
    setMenu(null);
    if (restoreFocus) anchor.current?.focus();
  }

  useLayoutEffect(() => {
    const element = panel.current;
    if (!menu || !element) return;
    element.showPopover();
    const bounds = element.getBoundingClientRect();
    element.style.left = `${Math.max(8, Math.min(menu.x, window.innerWidth - bounds.width - 8))}px`;
    element.style.top = `${Math.max(8, Math.min(menu.y, window.innerHeight - bounds.height - 8))}px`;
    element.querySelector<HTMLButtonElement>("button")?.focus();
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const dismiss = (event: Event) => {
      if (event.target instanceof Node && panel.current?.contains(event.target)) return;
      panel.current?.hidePopover();
      setMenu(null);
    };
    window.addEventListener("resize", dismiss);
    document.addEventListener("scroll", dismiss, true);
    return () => {
      window.removeEventListener("resize", dismiss);
      document.removeEventListener("scroll", dismiss, true);
    };
  }, [menu]);

  async function action(value: "reveal" | "open" | "resolve") {
    close(true);
    try {
      if (!desktop) throw new Error("请在桌面版中操作本地文件。");
      const resolved = invoke<string>("local_file_action", { filePath, workspacePath: workspacePath ?? null, action: value });
      if (value === "resolve") {
        // Start the clipboard write in the click gesture, before awaiting IPC (WebKit).
        if (typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
          await navigator.clipboard.write([new ClipboardItem({
            "text/plain": resolved.then(path => new Blob([path], { type: "text/plain" })),
          })]);
        } else {
          await navigator.clipboard.writeText(await resolved);
        }
      } else {
        await resolved;
      }
      const text = value === "resolve" ? "已复制完整路径" : value === "reveal" ? "已打开文件所在位置" : "已用默认应用打开";
      if (onNotify) onNotify(text);
      else setFeedback({ text, error: false });
    } catch (error) {
      const text = String(error).replace(/^Error: /, "");
      if (onNotify) onNotify(text);
      else setFeedback({ text, error: true });
    }
  }

  function menuKeys(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") { event.preventDefault(); close(true); return; }
    const items = Array.from(panel.current?.querySelectorAll<HTMLButtonElement>("[role='menuitem']") ?? []);
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === "ArrowDown" ? (index + 1) % items.length
      : event.key === "ArrowUp" ? (index - 1 + items.length) % items.length
      : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : -1;
    if (next >= 0) { event.preventDefault(); items[next]?.focus(); }
    if (event.key === "Tab") close();
  }

  return <>
    <a
      {...props}
      ref={anchor}
      className="message-file-link"
      title={`查看文件：${filePath}${line ? `:${line}` : ""} · 右键可打开所在位置`}
      aria-haspopup="menu"
      aria-controls={menu ? id : undefined}
      aria-expanded={!!menu}
      onClick={event => {
        event.preventDefault();
        event.stopPropagation();
        if (onOpenFile) onOpenFile(filePath, line);
        else void action("open");
      }}
      onContextMenu={event => {
        event.preventDefault();
        event.stopPropagation();
        setFeedback(null);
        setMenu({ x: event.clientX, y: event.clientY });
      }}
      onKeyDown={event => {
        if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
          event.preventDefault();
          const bounds = event.currentTarget.getBoundingClientRect();
          setMenu({ x: bounds.left, y: bounds.bottom + 4 });
        }
      }}
    >{children}</a>
    {feedback && <span className="file-link-feedback" role={feedback.error ? "alert" : "status"}>{feedback.text}</span>}
    {menu && createPortal(<div
      ref={panel}
      id={id}
      popover="auto"
      role="menu"
      aria-label="本地文件操作"
      className="file-link-menu"
      style={{ left: menu.x, top: menu.y }}
      onKeyDown={menuKeys}
      onToggle={event => { if (event.newState === "closed") setMenu(null); }}
    >
      <div className="file-link-menu-heading" title={filePath}><Icon name="file" size={14} /><span>{filePath.split(/[\\/]/).filter(Boolean).pop() ?? filePath}</span></div>
      <button type="button" role="menuitem" onClick={() => void action("reveal")}><Icon name="folder" size={16} />{revealLabel}</button>
      <button type="button" role="menuitem" onClick={() => void action("open")}><Icon name="external" size={16} />用默认应用打开</button>
      <div className="file-link-menu-divider" role="separator" />
      <button type="button" role="menuitem" onClick={() => void action("resolve")}><Icon name="copy" size={16} />复制完整路径</button>
    </div>, document.body)}
  </>;
}
