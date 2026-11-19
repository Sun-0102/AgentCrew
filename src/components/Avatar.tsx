import Icon from "./Icon";
export default function Avatar({ agent = "codex", small = false }: { agent?: string; small?: boolean }) {
  return <span className={`avatar avatar-${agent} ${small ? "avatar-small" : ""}`} aria-hidden="true">{agent === "group" ? <Icon name="group" size={small ? 19 : 23} /> : agent === "me" ? "我" : agent === "deepseek" ? "D" : "C"}</span>;
}
