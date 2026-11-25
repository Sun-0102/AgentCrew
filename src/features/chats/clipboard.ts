import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Copy plain text to the system clipboard with a DOM execCommand fallback
 * for WebView contexts where navigator.clipboard may be unavailable or denied.
 */
export async function copyTextToClipboard(text: string): Promise<boolean> {
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Fall through to legacy execCommand fallback
    }
  }

  if (typeof document === "undefined") return false;
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.top = "-9999px";
  textarea.style.left = "-9999px";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);

  const selection = document.getSelection();
  const savedRanges: Range[] = [];
  if (selection) {
    for (let i = 0; i < selection.rangeCount; i++) {
      savedRanges.push(selection.getRangeAt(i).cloneRange());
    }
  }

  let ok = false;
  try {
    textarea.focus({ preventScroll: true });
    textarea.select();
    textarea.setSelectionRange(0, text.length);
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  } finally {
    document.body.removeChild(textarea);
    if (selection) {
      selection.removeAllRanges();
      for (const range of savedRanges) {
        try {
          selection.addRange(range);
        } catch {
          // Ignore invalid restored range
        }
      }
    }
  }
  return ok;
}

export function useCopyToClipboard(options?: {
  timeoutMs?: number;
  onSuccess?: () => void;
  onError?: () => void;
}) {
  const timeoutMs = options?.timeoutMs ?? 1800;
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<number | null>(null);
  const onSuccessRef = useRef(options?.onSuccess);
  const onErrorRef = useRef(options?.onError);
  onSuccessRef.current = options?.onSuccess;
  onErrorRef.current = options?.onError;

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    };
  }, []);

  const copy = useCallback(async (text: string) => {
    const ok = await copyTextToClipboard(text);
    if (ok) {
      setCopied(true);
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => {
        setCopied(false);
        timerRef.current = null;
      }, timeoutMs);
      onSuccessRef.current?.();
      return true;
    }
    onErrorRef.current?.();
    return false;
  }, [timeoutMs]);

  return { copied, copy };
}

function serializeChildren(node: Node, listDepth = 0): string {
  let out = "";
  node.childNodes.forEach(child => {
    out += serializeNode(child, listDepth);
  });
  return out;
}

function serializeNode(node: Node, listDepth = 0): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? "";
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as HTMLElement;
  if (el.dataset.copyIgnore === "true") return "";

  const tag = el.tagName.toLowerCase();
  if (tag === "br") return "\n";
  if (tag === "hr") return "\n---\n\n";

  if (tag === "pre") {
    const codeEl = el.querySelector("code");
    const className = codeEl?.className ?? "";
    const langMatch = className.match(/\blanguage-([a-z0-9_+-]+)\b/i);
    const lang = langMatch?.[1] ?? "";
    const raw = (codeEl?.textContent ?? el.textContent ?? "").replace(/\n$/, "");
    return `\n\`\`\`${lang}\n${raw}\n\`\`\`\n\n`;
  }

  if (tag === "code") {
    const text = el.textContent ?? "";
    if (!text) return "";
    const ticks = text.includes("`") ? "``" : "`";
    return `${ticks}${text}${ticks}`;
  }

  if (tag === "strong" || tag === "b") {
    const inner = serializeChildren(el, listDepth);
    return inner ? `**${inner}**` : "";
  }
  if (tag === "em" || tag === "i") {
    const inner = serializeChildren(el, listDepth);
    return inner ? `*${inner}*` : "";
  }
  if (tag === "del" || tag === "s") {
    const inner = serializeChildren(el, listDepth);
    return inner ? `~~${inner}~~` : "";
  }
  if (tag === "a") {
    const inner = serializeChildren(el, listDepth).trim();
    const href = el.getAttribute("href");
    if (!href) return inner;
    return inner && inner !== href ? `[${inner}](${href})` : href;
  }

  if (/^h[1-6]$/.test(tag)) {
    const level = Number(tag[1]);
    const inner = serializeChildren(el, listDepth).trim();
    return inner ? `${"#".repeat(level)} ${inner}\n\n` : "";
  }

  if (tag === "blockquote") {
    const inner = serializeChildren(el, listDepth).trim();
    if (!inner) return "";
    return `${inner.split("\n").map(line => `> ${line}`).join("\n")}\n\n`;
  }

  if (tag === "ul" || tag === "ol") {
    const ordered = tag === "ol";
    const lines: string[] = [];
    let index = 1;
    el.childNodes.forEach(child => {
      if (child.nodeType !== Node.ELEMENT_NODE) return;
      const item = child as HTMLElement;
      if (item.tagName.toLowerCase() !== "li") return;
      const indent = "  ".repeat(listDepth);
      const prefix = ordered ? `${index++}. ` : "- ";
      const content = serializeChildren(item, listDepth + 1).trim();
      if (content) {
        const indented = content.replace(/\n/g, `\n${indent}  `);
        lines.push(`${indent}${prefix}${indented}`);
      }
    });
    return lines.length ? `${lines.join("\n")}\n\n` : "";
  }

  if (tag === "table") {
    const rows: string[][] = [];
    el.querySelectorAll("tr").forEach(tr => {
      const cells: string[] = [];
      tr.querySelectorAll("th, td").forEach(cell => {
        cells.push(serializeChildren(cell, listDepth).trim().replace(/\|/g, "\\|").replace(/\n+/g, " "));
      });
      if (cells.length) rows.push(cells);
    });
    if (!rows.length) return "";
    const [head, ...body] = rows;
    const headerLine = `| ${head.join(" | ")} |`;
    const sepLine = `| ${head.map(() => "---").join(" | ")} |`;
    const bodyLines = body.map(row => `| ${row.join(" | ")} |`);
    return `${[headerLine, sepLine, ...bodyLines].join("\n")}\n\n`;
  }

  if (tag === "p") {
    const inner = serializeChildren(el, listDepth).trim();
    return inner ? `${inner}\n\n` : "";
  }

  return serializeChildren(el, listDepth);
}

/**
 * Serialize a user text selection inside a rendered markdown container into clean Markdown,
 * skipping UI chrome marked with data-copy-ignore="true".
 * Returns null if the selection is plain inline text or inside a single code block so the browser
 * handles normal plain-text copying natively.
 */
export function serializeMarkdownSelection(container: HTMLElement): string | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.commonAncestorContainer)) return null;

  // If the entire selection is inside a single <code>/<pre>, let native plain-text copy handle it.
  const ancestorEl =
    range.commonAncestorContainer.nodeType === Node.ELEMENT_NODE
      ? (range.commonAncestorContainer as HTMLElement)
      : range.commonAncestorContainer.parentElement;
  if (ancestorEl?.closest("pre")) {
    return null;
  }

  const fragment = range.cloneContents();
  const wrapper = document.createElement("div");
  wrapper.appendChild(fragment);

  // Strip UI chrome (like code block headers/copy buttons) from the cloned selection.
  const ignored = wrapper.querySelectorAll("[data-copy-ignore='true']");
  const hasIgnoredChrome = ignored.length > 0;
  ignored.forEach(node => node.remove());

  const hasRichStructure = Boolean(
    wrapper.querySelector("pre, blockquote, ul, ol, h1, h2, h3, h4, h5, h6, table, code, strong, em, del, hr"),
  );
  if (!hasRichStructure && !hasIgnoredChrome) return null;

  const serialized = serializeChildren(wrapper)
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return serialized || null;
}
