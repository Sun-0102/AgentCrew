import Markdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { Children, isValidElement, memo, useCallback, useEffect, useMemo, useState, type ClipboardEvent, type ComponentProps, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createPortal } from "react-dom";
import LocalFileLink from "./LocalFileLink";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { desktop, useAgents } from "./api";
import { serializeMarkdownSelection, useCopyToClipboard } from "./clipboard";
import { highlightCode, highlightCodeSync } from "./code-highlight";
import { remarkMentions, remarkSkills } from "./mention-markdown";

const localImage = /^ai-gent-image:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;
const localAttachment = /^ai-gent-attachment:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;

function parseLocalFileLink(href: string, labelText?: string): { path: string; line?: number } | null {
  const raw = href.trim();
  if (!raw || raw.startsWith("#")) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw) && !/^file:\/\//i.test(raw) && !/^[a-z]:[\\/]/i.test(raw)) return null;

  let path = raw;
  if (path.startsWith("file://")) {
    path = path.slice("file://".length);
  }
  let line: number | undefined;
  const hashMatch = path.match(/#L(\d+)(?:-L?\d+)?$/i);
  if (hashMatch) {
    line = Number(hashMatch[1]);
    path = path.slice(0, hashMatch.index);
  } else {
    const colonMatch = path.match(/:(\d+)(?::\d+)?$/);
    if (colonMatch) {
      line = Number(colonMatch[1]);
      path = path.slice(0, colonMatch.index);
    }
  }
  if (!line && labelText) {
    const labelLine = labelText.trim().match(/:(\d+)(?:-\d+)?$/);
    if (labelLine) line = Number(labelLine[1]);
  }
  try {
    path = decodeURIComponent(path);
  } catch {
    // Keep raw path if URI decoding fails
  }
  return path ? { path, line: Number.isFinite(line) ? line : undefined } : null;
}

function parseLocalImagePath(src: string): string | null {
  const raw = src.trim();
  if (!raw || raw.startsWith("#") || raw.startsWith("//")) return null;
  if (localImage.test(raw) || localAttachment.test(raw)) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw) && !/^file:\/\//i.test(raw) && !/^[a-z]:[\\/]/i.test(raw)) return null;

  let path = raw;
  if (/^file:\/\//i.test(path)) {
    path = path.replace(/^file:\/\/(?:localhost)?/i, "");
    if (/^\/[a-z]:[\\/]/i.test(path)) {
      path = path.slice(1);
    }
  }
  path = path.split(/[?#]/, 1)[0] ?? "";
  try {
    path = decodeURIComponent(path);
  } catch {
    // Keep raw path if URI decoding fails
  }
  return path.trim() || null;
}

function PreviewImage({ src, alt, ...props }: ComponentProps<"img">) {
  const [open, setOpen] = useState(false);
  return <><button type="button" className="message-image-thumbnail" aria-label={`放大图片：${alt || "图片"}`} aria-haspopup="dialog" onClick={event => { event.preventDefault(); event.stopPropagation(); setOpen(true); }}>
    <img {...props} src={src} alt={alt} decoding="async" />
  </button>{open && createPortal(<Dialog title="图片预览" onClose={() => setOpen(false)}><div className="image-preview"><img src={src} alt={alt} decoding="async" /></div></Dialog>, document.body)}</>;
}

function LocalImage({ id, alt, command = "image_read" }: { id: string; alt: string; command?: "image_read" | "attachment_read" }) {
  const [image, setImage] = useState<{ id: string; url?: string; error?: string }>();
  useEffect(() => {
    let active = true;
    invoke<string>(command, { id }).then(
      url => { if (active) setImage({ id, url }); },
      () => { if (active) setImage({ id, error: "图片读取失败" }); },
    );
    return () => { active = false; };
  }, [id, command]);
  return image?.id === id && image.url
    ? <PreviewImage src={image.url} alt={alt} />
    : <span role="status">{image?.id === id && image.error ? image.error : "正在读取图片…"}</span>;
}

function LocalFileImage({
  filePath,
  workspacePath,
  alt,
  onOpenFile,
  onNotify,
}: {
  filePath: string;
  workspacePath?: string;
  alt: string;
  onOpenFile?: (path: string, line?: number) => void;
  onNotify?: (message: string) => void;
}) {
  const cacheKey = `${workspacePath ?? ""}\0${filePath}`;
  const [image, setImage] = useState<{ key: string; url?: string; error?: string }>();
  useEffect(() => {
    if (!desktop) {
      setImage({ key: cacheKey, error: "仅桌面版支持加载本地图片" });
      return;
    }
    let active = true;
    invoke<string>("local_image_read", {
      filePath,
      workspacePath: workspacePath ?? null,
    }).then(
      url => { if (active) setImage({ key: cacheKey, url }); },
      cause => {
        const detail = String(cause).replace(/^Error:\s*/, "").trim();
        if (active) setImage({ key: cacheKey, error: detail || "图片读取失败" });
      },
    );
    return () => { active = false; };
  }, [cacheKey, filePath, workspacePath]);

  if (image?.key === cacheKey && image.url) {
    return <PreviewImage src={image.url} alt={alt} />;
  }
  if (image?.key === cacheKey && image.error) {
    const fileName = filePath.split(/[\\/]/).filter(Boolean).pop() ?? filePath;
    return (
      <span className="message-image-fallback" role="status" title={`${filePath}：${image.error}`}>
        <span>{alt || fileName}（{image.error}）</span>
        <LocalFileLink filePath={filePath} workspacePath={workspacePath} onOpenFile={onOpenFile} onNotify={onNotify}>
          {fileName}
        </LocalFileLink>
      </span>
    );
  }
  return <span className="message-image-status" role="status">正在读取图片…</span>;
}

interface DispatchItem {
  to: string;
  instruction: string;
  return_to_sender?: boolean;
}

function parseDispatchItems(raw: string): DispatchItem[] | null {
  try {
    const parsed = JSON.parse(raw.trim()) as unknown;
    const list = Array.isArray(parsed) ? parsed : [parsed];
    const items = list.filter(
      (item): item is DispatchItem =>
        Boolean(item) &&
        typeof item === "object" &&
        typeof (item as DispatchItem).to === "string" &&
        typeof (item as DispatchItem).instruction === "string" &&
        (item as DispatchItem).instruction.trim().length > 0,
    );
    return items.length > 0 ? items : null;
  } catch {
    return null;
  }
}

function DispatchCards({ raw }: { raw: string }) {
  const registry = useAgents();
  const items = parseDispatchItems(raw);
  if (!items) {
    return <div className="message-dispatch-card message-dispatch-pending">正在整理派发指令…</div>;
  }
  return <div className="message-dispatch-list">{items.map((item, index) => {
    const targetKey = item.to.trim().replace(/^@/, "").toLowerCase();
    const agent = registry.find(a => a.id.toLowerCase() === targetKey || a.name.toLowerCase() === targetKey);
    const label = agent ? agent.name : item.to.trim().replace(/^@/, "");
    return <div className="message-dispatch-card" key={`${targetKey}-${index}`}>
      <div className="message-dispatch-head">
        <span className="message-mention">@{label}</span>
        <span className="message-dispatch-badge">派发任务</span>
        {item.return_to_sender !== false && <span className="message-dispatch-return">完成后回传验收</span>}
      </div>
      <div className="message-dispatch-body">{item.instruction}</div>
    </div>;
  })}</div>;
}

function extractPlainText(node: unknown): string {
  if (typeof node === "string") return node;
  if (Array.isArray(node)) return node.map(extractPlainText).join("");
  if (isValidElement<{ children?: unknown }>(node)) return extractPlainText(node.props.children);
  return "";
}

function MarkdownCodeBlock({
  language,
  code,
  children,
  preProps,
  onNotify,
}: {
  language?: string;
  code: string;
  children: ReactNode;
  preProps: ComponentProps<"pre">;
  onNotify?: (message: string) => void;
}) {
  const { copied, copy } = useCopyToClipboard({
    onError: () => onNotify?.("复制失败，请选择文字后手动复制"),
  });
  const syncHighlightedHtml = highlightCodeSync(code, language);
  const [asyncHighlight, setAsyncHighlight] = useState<{
    code: string;
    language?: string;
    html: string;
  } | null>(null);

  useEffect(() => {
    if (syncHighlightedHtml !== null) return;
    let cancelled = false;
    void highlightCode(code, language).then(html => {
      if (!cancelled && html) {
        setAsyncHighlight({ code, language, html });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [code, language, syncHighlightedHtml]);

  const highlightedHtml =
    syncHighlightedHtml ??
    (asyncHighlight && asyncHighlight.code === code && asyncHighlight.language === language
      ? asyncHighlight.html
      : null);

  return (
    <div className="message-code-block">
      <div className="message-code-header" data-copy-ignore="true">
        <span className="message-code-lang">{language || "code"}</span>
        <button
          type="button"
          className={`message-code-copy ${copied ? "is-copied" : ""}`}
          aria-label={copied ? "已复制代码" : "复制代码"}
          title={copied ? "已复制" : "复制代码"}
          onClick={() => void copy(code)}
        >
          <Icon name={copied ? "check" : "copy"} size={13} />
          <span>{copied ? "已复制" : "复制"}</span>
        </button>
      </div>
      {highlightedHtml ? (
        <div
          className="message-code-highlighted"
          dangerouslySetInnerHTML={{ __html: highlightedHtml }}
        />
      ) : (
        <pre {...preProps}>{children}</pre>
      )}
    </div>
  );
}

export default memo(function MessageBody({
  text,
  members,
  onOpenFile,
  workspacePath,
  onNotify,
}: {
  text: string;
  members?: string[];
  onOpenFile?: (path: string, line?: number) => void;
  workspacePath?: string;
  onNotify?: (message: string) => void;
}) {
  const registry = useAgents();
  const mentionAgents = members ? registry.filter(agent => members.includes(agent.id)) : registry;
  const handleCopy = useCallback((event: ClipboardEvent<HTMLDivElement>) => {
    const markdown = serializeMarkdownSelection(event.currentTarget);
    if (!markdown) return;
    event.preventDefault();
    event.clipboardData.setData("text/plain", markdown);
  }, []);
  const components = useMemo<NonNullable<ComponentProps<typeof Markdown>["components"]>>(
    () => ({
      table: ({ children }) => <div className="message-table" role="region" aria-label="消息表格" tabIndex={0}><table>{children}</table></div>,
      pre: ({ node: _node, children, ...props }) => {
        const child = Children.toArray(children)[0];
        if (isValidElement<{ className?: string; children?: unknown }>(child)) {
          const className = typeof child.props.className === "string" ? child.props.className : "";
          const classes = className.split(/\s+/).filter(Boolean);
          if (classes.includes("language-agentcrew-dispatch")) {
            const raw = extractPlainText(child.props.children);
            return <DispatchCards raw={raw} />;
          }
          const langClass = classes.find(token => token.startsWith("language-"));
          const language = langClass ? langClass.slice("language-".length) : undefined;
          const rawCode = extractPlainText(child.props.children).replace(/\n$/, "");
          return (
            <MarkdownCodeBlock language={language} code={rawCode} preProps={props} onNotify={onNotify}>
              {children}
            </MarkdownCodeBlock>
          );
        }
        const rawCode = extractPlainText(children).replace(/\n$/, "");
        return (
          <MarkdownCodeBlock code={rawCode} preProps={props} onNotify={onNotify}>
            {children}
          </MarkdownCodeBlock>
        );
      },
      a: ({ node: _node, href, children, onClick, ...props }) => {
        const parsed = href ? parseLocalFileLink(href, extractPlainText(children)) : null;
        if (parsed) {
          return <LocalFileLink {...props} href={href} filePath={parsed.path} line={parsed.line} workspacePath={workspacePath} onOpenFile={onOpenFile} onNotify={onNotify}>{children}</LocalFileLink>;
        }
        const isHttp = !!href && /^https?:\/\//i.test(href);
        return (
          <a
            {...props}
            href={href}
            target="_blank"
            rel="noopener noreferrer"
            onClick={event => {
              onClick?.(event);
              if (event.defaultPrevented || !isHttp || !desktop) return;
              event.preventDefault();
              void invoke("git_open_url", { url: href }).catch(cause => {
                onNotify?.(`无法打开链接：${String(cause)}`);
              });
            }}
          >
            {children}
          </a>
        );
      },
      img: ({ node: _node, src, alt, ...props }) => {
        const imageId = src?.match(localImage)?.[1];
        if (imageId) return <LocalImage id={imageId} alt={alt ?? "生成的图片"} command="image_read" />;
        const attachmentId = src?.match(localAttachment)?.[1];
        if (attachmentId) return <LocalImage id={attachmentId} alt={alt ?? "图片附件"} command="attachment_read" />;
        const localFilePath = src ? parseLocalImagePath(src) : null;
        if (localFilePath) {
          return <LocalFileImage filePath={localFilePath} workspacePath={workspacePath} alt={alt ?? "本地图片"} onOpenFile={onOpenFile} onNotify={onNotify} />;
        }
        return src ? <PreviewImage {...props} src={src} alt={alt} /> : <span>{alt ?? "图片不可用"}</span>;
      },
    }),
    [onOpenFile, workspacePath, onNotify],
  );
  return <div className="message-body" onCopy={handleCopy}><Markdown remarkPlugins={[remarkGfm, [remarkSkills, { source: text }], [remarkMentions, { agents: mentionAgents, source: text }]]} urlTransform={(url, key) => {
    if (key === "src" && (
      localImage.test(url)
      || localAttachment.test(url)
      || /^data:image\/(png|jpeg|webp|gif|bmp);base64,[A-Za-z0-9+/]+={0,2}$/.test(url)
      || /^file:\/\//i.test(url)
      || /^[a-z]:[\\/]/i.test(url)
    )) {
      return url;
    }
    if (key === "href" && url.startsWith("file://")) {
      return url;
    }
    return defaultUrlTransform(url);
  }} components={components}>{text}</Markdown></div>;
});
