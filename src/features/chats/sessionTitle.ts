const MAX_TITLE_CHARS = 24;

function truncateChars(value: string, max: number): string {
  const chars = [...value];
  if (chars.length <= max) return value;
  return `${chars.slice(0, max - 1).join("").trimEnd()}…`;
}

function stripLeadingDirectives(text: string): string {
  let current = text.trim();
  // Strip leading @mention or /skill tokens only when meaningful body text follows.
  while (true) {
    const match = current.match(/^(?:@[^\s@]+|\/[A-Za-z0-9_.:-]+)\s+(.+)$/);
    if (!match) break;
    current = match[1].trim();
  }
  return current;
}

export function deriveSessionTitle(
  rawText: string,
  files: ReadonlyArray<{ name: string; isImage: boolean }> = [],
): string {
  const normalized = rawText.replace(/\s+/g, " ").trim();
  const body = normalized ? stripLeadingDirectives(normalized) : "";
  if (body) {
    return truncateChars(body, MAX_TITLE_CHARS);
  }
  if (files.length === 1) {
    const [file] = files;
    const prefix = file.isImage ? "图片：" : "附件：";
    const name = file.name.trim();
    return name ? truncateChars(`${prefix}${name}`, MAX_TITLE_CHARS) : (file.isImage ? "查看图片" : "查看附件");
  }
  if (files.length > 1) {
    const allImages = files.every(file => file.isImage);
    return allImages ? `查看 ${files.length} 张图片` : `查看 ${files.length} 个附件`;
  }
  return "新会话";
}
