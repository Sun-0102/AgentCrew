import { useCallback, useEffect, useState } from "react";
import {
  emptyModelDefaults,
  getModelDefaults,
  saveModelDefault,
  setGroupFollowPrivate,
  type ModelDefaultScope,
  type ModelDefaultsConfig,
} from "../features/chats/modelDefaults";

export function readStoredRecord<T>(key: string): Record<string, T> {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "{}");
    return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, T>) : {};
  } catch {
    return {};
  }
}

export function useWorkbenchPreferences(onError: (message: string) => void) {
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    try {
      const saved = localStorage.getItem("ai-gent.theme.v1");
      if (saved === "dark" || saved === "light") return saved;
    } catch {}
    return "light";
  });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("ai-gent.theme.v1", theme);
    } catch {}
  }, [theme]);

  const [modelDefaults, setModelDefaults] = useState<ModelDefaultsConfig>(emptyModelDefaults);

  useEffect(() => {
    let active = true;
    void getModelDefaults()
      .then(cfg => {
        if (active) setModelDefaults(cfg);
      })
      .catch(cause => {
        if (active) onError(`读取默认模型配置失败：${String(cause)}`);
      });
    return () => {
      active = false;
    };
  }, [onError]);

  const handleSaveModelDefault = useCallback(
    async (params: {
      scope: ModelDefaultScope;
      agentId: string;
      model: string;
      reasoningEffort?: string | null;
      serviceTier?: string | null;
    }) => {
      const next = await saveModelDefault(params);
      setModelDefaults(next);
    },
    [],
  );

  const handleToggleGroupFollowPrivate = useCallback(async (follow: boolean) => {
    const next = await setGroupFollowPrivate(follow);
    setModelDefaults(next);
  }, []);

  return {
    theme,
    setTheme,
    modelDefaults,
    handleSaveModelDefault,
    handleToggleGroupFollowPrivate,
  };
}
