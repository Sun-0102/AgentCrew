import type { Model } from "./api";

const labels: Record<string, string> = { default: "标准", priority: "快速", fast: "快速", ultrafast: "极速" };

export function speedChoices(model: Model | undefined) {
  return [{ id: "default", name: "标准", description: "标准速度" },
    ...(model?.serviceTiers ?? []).filter(t => t.id !== "default").map(t => ({ ...t, name: labels[t.id] ?? t.name }))];
}

export function selectedSpeed(model: Model | undefined, saved?: string): string {
  return speedChoices(model).find(t => t.id === saved)?.id ?? "default";
}
