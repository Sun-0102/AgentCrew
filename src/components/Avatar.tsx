import Icon from "./Icon";
import codexIcon from "../assets/agents/codex.svg";
import deepseekIcon from "../assets/agents/deepseek.svg";
export default function Avatar({ agent = "codex", small = false }: { agent?: string; small?: boolean }) {
  return <span className={`avatar avatar-${agent} ${small ? "avatar-small" : ""}`} aria-hidden="true">{agent === "group" ? <Icon name="group" size={small ? 19 : 23} /> : agent === "me" ? "我" : <img className="agent-logo" src={agent === "deepseek" ? deepseekIcon : codexIcon} alt="" />}</span>;
}
