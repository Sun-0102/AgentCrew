import { providerKind, useAgents } from "../features/chats/api";
import Icon from "./Icon";
import codexIcon from "../assets/agents/codex.svg";
import deepseekIcon from "../assets/agents/deepseek.svg";
import geminiIcon from "../assets/agents/gemini.svg";
export default function Avatar({ agent = "codex", small = false }: { agent?: string; small?: boolean }) {
  useAgents();
  const provider = providerKind(agent);
  const icon = provider === "deepseek" ? deepseekIcon : provider === "gemini" ? geminiIcon : codexIcon;
  return <span className={`avatar avatar-${provider} ${small ? "avatar-small" : ""}`} aria-hidden="true">{agent === "group" ? <Icon name="group" size={small ? 19 : 23} /> : agent === "me" ? "我" : <img className="agent-logo" src={icon} alt="" />}</span>;
}
