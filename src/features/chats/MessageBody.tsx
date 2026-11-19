import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function MessageBody({ text }: { text: string }) {
  return <div className="message-body"><Markdown remarkPlugins={[remarkGfm]} components={{
    table: ({ children }) => <div className="message-table" role="region" aria-label="消息表格" tabIndex={0}><table>{children}</table></div>,
    a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
  }}>{text}</Markdown></div>;
}
