import type { Model } from "./api";

export const effortLabels: Record<string, string> = { off: "关闭思考", none: "不思考", minimal: "极低", low: "低", medium: "中", high: "高", xhigh: "超高", max: "最高", ultra: "极高" };

export function selectedEffort(model: Model | undefined, saved?: string): string {
  const choices = model?.supportedReasoningEfforts ?? [];
  return choices.find(c => c.reasoningEffort === saved)?.reasoningEffort
    ?? choices.find(c => c.reasoningEffort === model?.defaultReasoningEffort)?.reasoningEffort
    ?? choices[0]?.reasoningEffort ?? "";
}
