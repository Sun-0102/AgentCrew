/**
 * Convert raw Markdown message text into clean single-line plain text
 * suitable for the compact reply-quote preview banner above the composer.
 */
export function formatReplyPreview(markdown: string, maxLength = 140): string {
  if (!markdown) return "";
  let text = markdown
    // Replace fenced code blocks with a concise placeholder
    .replace(/```[\s\S]*?```/g, " [代码块] ")
    // Images: ![alt](url) -> alt or [图片]
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, (_, alt: string) => (alt.trim() ? `[图片: ${alt.trim()}]` : "[图片]"))
    // Links: [label](url) -> label
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    // Inline code: `code` -> code
    .replace(/`([^`]+)`/g, "$1")
    // Bold / italic / strikethrough markers
    .replace(/(\*\*|__)(.*?)\1/g, "$2")
    .replace(/~~(.*?)~~/g, "$1")
    .replace(/(^|[\s(（【「『])\*([^*]+)\*(?=[\s).,，。！？；：）】」』]|$)/g, "$1$2")
    // Strip leading heading / blockquote / list prefixes per line
    .replace(/^\s{0,3}(?:#{1,6}\s+|>\s*|[-*+]\s+|\d+\.\s+)/gm, "")
    // Collapse whitespace & newlines into single spaces
    .replace(/\s+/g, " ")
    .trim();

  if (text.length > maxLength) {
    text = `${text.slice(0, maxLength).trimEnd()}…`;
  }
  return text;
}
