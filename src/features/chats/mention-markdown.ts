import type { Agent } from "./api";
import { skillCommandSpans } from "./skills";

interface Node {
  type: string;
  value?: string;
  children?: Node[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  data?: { hName: string; hProperties: Record<string, string> };
}

interface MentionOptions { agents: Agent[]; source: string }

// Decorate Markdown text only. Mention styling does not dispatch messages.
export function remarkMentions({ agents, source }: MentionOptions) {
  const targets = agents.flatMap(agent => {
    const unique = agents.filter(other => other.name.toLowerCase() === agent.name.toLowerCase()).length === 1;
    return (unique ? [agent.id, agent.name] : [agent.id]).map(label => ({ label, name: agent.name, self: false }));
  });
  targets.unshift(...["我", "你", "用户", "user"].map(label => ({ label, name: "当前用户", self: true })));
  targets.sort((a, b) => b.label.length - a.label.length);
  const closingQuotes: Record<string, string> = { "「": "」", "『": "』", "“": "”", "‘": "’", '"': '"', "'": "'" };
  return (tree: Node) => {
    const visit = (node: Node, quotes: string[] = []) => {
      if (["blockquote", "code", "inlineCode", "link", "image", "html", "skill"].includes(node.type) || !node.children) return;
      // Quote state follows inline formatting, but never crosses paragraph boundaries.
      const state = ["paragraph", "heading", "tableCell"].includes(node.type) ? [] : quotes;
      node.children = node.children.flatMap(child => {
        if (child.type !== "text" || !child.value) { visit(child, state); return [child]; }
        const text = child.value;
        const offset = child.position?.start.offset;
        const end = child.position?.end.offset;
        // Escapes/entities must stay literal; raw offsets are not interchangeable with decoded text.
        if (offset === undefined || end === undefined || source.slice(offset, end) !== text) return [child];
        const parts: Node[] = [];
        let start = 0;
        for (let cursor = 0; cursor < text.length; cursor++) {
          const character = text[cursor];
          if (state.at(-1) === character) { state.pop(); continue; }
          const closing = closingQuotes[character];
          if (closing && (character !== "'" || !/[a-z0-9]/i.test(text[cursor - 1] ?? ""))) state.push(closing);
          if (state.length || character !== "@" || /[a-z0-9_@\\]/i.test(source[offset + cursor - 1] ?? "")) continue;
          const target = targets.find(target => text.slice(cursor + 1, cursor + 1 + target.label.length).toLowerCase() === target.label.toLowerCase()
            && !/[\p{L}\p{N}_-]/u.test(source[offset + cursor + target.label.length + 1] ?? ""));
          if (!target) continue;
          if (cursor > start) parts.push({ type: "text", value: text.slice(start, cursor) });
          const mention = text.slice(cursor, cursor + target.label.length + 1);
          parts.push({ type: "mention", data: { hName: "span", hProperties: {
            className: `message-mention${target.self ? " message-mention-self" : ""}`,
            title: target.self ? "当前用户" : `${target.name} · 智能体`,
          } }, children: [{ type: "text", value: mention }] });
          cursor += target.label.length;
          start = cursor + 1;
        }
        if (!parts.length) return [child];
        if (start < text.length) parts.push({ type: "text", value: text.slice(start) });
        return parts;
      });
    };
    visit(tree);
  };
}

export function remarkSkills({ source }: { source: string }) {
  const spans = skillCommandSpans(source);
  return (tree: Node) => {
    if (!spans.length) return;
    const visit = (node: Node) => {
      if (["blockquote", "code", "inlineCode", "link", "image", "html", "mention", "skill"].includes(node.type) || !node.children) return;
      node.children = node.children.flatMap(child => {
        if (child.type !== "text" || !child.value) { visit(child); return [child]; }
        const text = child.value;
        const offset = child.position?.start.offset;
        const end = child.position?.end.offset;
        if (offset === undefined || end === undefined || source.slice(offset, end) !== text) return [child];
        const overlapping = spans.filter(span => span.start >= offset && span.end <= end);
        if (!overlapping.length) return [child];
        const parts: Node[] = [];
        let cursor = 0;
        for (const span of overlapping) {
          const localStart = span.start - offset;
          const localEnd = span.end - offset;
          if (localStart < cursor) continue;
          if (localStart > cursor) {
            parts.push({
              type: "text",
              value: text.slice(cursor, localStart),
              position: { start: { offset: offset + cursor }, end: { offset: offset + localStart } },
            });
          }
          parts.push({
            type: "skill",
            data: {
              hName: "span",
              hProperties: {
                className: "message-skill",
                "data-skill": span.name,
                title: `技能：${span.name}`,
              },
            },
            children: [{ type: "text", value: text.slice(localStart, localEnd) }],
          });
          cursor = localEnd;
        }
        if (cursor < text.length) {
          parts.push({
            type: "text",
            value: text.slice(cursor),
            position: { start: { offset: offset + cursor }, end: { offset: end } },
          });
        }
        return parts;
      });
    };
    visit(tree);
  };
}
