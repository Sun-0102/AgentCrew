import { providerKind, useAgents } from "../features/chats/api";
import Icon from "./Icon";
import codexIcon from "../assets/agents/codex.svg";
import deepseekIcon from "../assets/agents/deepseek.svg";
export default function Avatar({ agent = "codex", small = false }: { agent?: string; small?: boolean }) {
  useAgents();
  const provider = providerKind(agent);
  return <span className={`avatar avatar-${provider} ${small ? "avatar-small" : ""}`} aria-hidden="true">{agent === "group" ? <Icon name="group" size={small ? 19 : 23} /> : agent === "me" ? "我" : provider === "gemini" ? <Icon name="spark" size={small ? 19 : 23} /> : <img className="agent-logo" src={provider === "deepseek" ? deepseekIcon : codexIcon} alt="" />}</span>;
}
