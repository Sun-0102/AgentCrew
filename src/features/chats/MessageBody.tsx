import { Fragment, type ReactNode } from "react";
function inline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, index) => part.startsWith("**") ? <strong key={index}>{part.slice(2, -2)}</strong> : part.startsWith("`") ? <code key={index}>{part.slice(1, -1)}</code> : <Fragment key={index}>{part}</Fragment>);
}
export default function MessageBody({ text }: { text: string }) {
  return <div className="message-body">{text.split(/(```[\s\S]*?(?:```|$))/g).map((block, index) => block.startsWith("```") ? <pre key={index}><code>{block.replace(/^```[^\n]*\n?/, "").replace(/```$/, "")}</code></pre> : <div key={index}>{block.split("\n").map((line, i) => line.startsWith("> ") ? <blockquote key={i}>{inline(line.slice(2))}</blockquote> : <div key={i} className={line.startsWith("- ") ? "message-list-line" : ""}>{inline(line.replace(/^#{1,4} /, "")) || <br />}{!line && <br />}</div>)}</div>)}</div>;
}
