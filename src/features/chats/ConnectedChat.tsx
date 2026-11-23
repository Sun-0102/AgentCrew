import { useEffect, useRef, useState, type ComponentProps } from "react";
import { invoke } from "@tauri-apps/api/core";
import ChatStage from "./ChatStage";
import { desktop, refreshAgents, useAgents, type Probe } from "./api";
import { selectedEffort } from "./reasoning";
import { selectedSpeed } from "./speed";

type Props = Pick<ComponentProps<typeof ChatStage>, "scope" | "scopeKey" | "title" | "group" | "project" | "agent" | "snapshot" | "onAgent" | "onInvite" | "onSettings" | "onRefresh" | "notify" | "session" | "onGroups" | "onRenameSession" | "onArchiveSession" | "sessionSaving" | "onDeleteSession" | "onGroupUpdated"> & { legacyProbes: Partial<Record<string, Probe | null>>; onConnectLegacy: Partial<Record<string, () => void>>; legacyConnecting: Partial<Record<string, boolean>> };
interface Preferences { models: Record<string,string>; efforts: Record<string,string>; tiers: Record<string,string> }

const probeCacheKey = "ai-gent.probe-cache.v1";

function readProbeCache(): Record<string, Probe> {
  try {
    const raw = JSON.parse(localStorage.getItem(probeCacheKey) ?? "{}");
    return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  } catch {
    return {};
  }
}

export default function ConnectedChat(p: Props) {
  const registry = useAgents();
  const [probes, setProbes] = useState<Record<string,Probe>>(readProbeCache);
  const [connecting, setConnecting] = useState<Record<string,boolean>>({});
  const [error, setError] = useState("");
  const [preferences, setPreferences] = useState<Preferences>(() => {
    try {
      const oldEfforts=JSON.parse(localStorage.getItem("ai-gent.reasoning.v1") ?? "{}");
      const oldTiers=Object.fromEntries(Object.entries(JSON.parse(localStorage.getItem("ai-gent.speed.v1") ?? "{}") as Record<string,string>).map(([model,tier])=>[`codex:${model}`,tier]));
      return {models:{},efforts:oldEfforts,tiers:oldTiers,...JSON.parse(localStorage.getItem("ai-gent.instance-models.v1") ?? "{}")};
    }
    catch { return {models:{},efforts:{},tiers:{}}; }
  });
  const attempted = useRef(new Set<string>());
  const agent = p.agent ?? "codex";
  const ids = p.group?.members ?? [agent];
  const allProbes: Record<string,Probe | null | undefined> = {...probes,...Object.fromEntries(Object.entries(p.legacyProbes).filter(([,probe]) => probe))};
  async function connect(id: string) {
    const connectLegacy=p.onConnectLegacy[id];
    if (connectLegacy) {connectLegacy();return;}
    const hasCached = !!probes[id]?.models?.length;
    if (!hasCached) setConnecting(current => ({...current,[id]:true}));
    setError("");
    try {
      const probe=await invoke<Probe>("agent_probe",{agent:id});
      setProbes(current=>{
        const next={...current,[id]:probe};
        try { localStorage.setItem(probeCacheKey, JSON.stringify(next)); } catch {}
        return next;
      });
      await refreshAgents();
    }
    catch(cause) {setError(String(cause));}
    finally {if (!hasCached) setConnecting(current=>({...current,[id]:false}));}
  }
  useEffect(() => {
    if (!desktop) return;
    void (async () => {
      for (const id of ids) {
        if (p.onConnectLegacy[id] || attempted.current.has(id) || !registry.some(a=>a.id===id && a.available)) continue;
        attempted.current.add(id); await connect(id);
      }
    })();
  }, [ids.join(","),registry]);
  function save(next: Preferences) {
    setPreferences(next);
    try {localStorage.setItem("ai-gent.instance-models.v1",JSON.stringify(next));}
    catch {setError("模型设置未能保存，重启后需要重新选择。");}
  }
  const models: Record<string,string> = {}, efforts: Record<string,string> = {}, tiers: Record<string,string> = {};
  for (const id of ids) {
    const probe=allProbes[id];
    const saved=preferences.models[id] ?? p.snapshot.sessions?.[id]?.model ?? (id===agent ? p.snapshot.session?.model : undefined);
    models[id] = probe?.models.find(m=>m.model===saved)?.model ?? probe?.models.find(m=>m.isDefault)?.model ?? probe?.models[0]?.model ?? "";
    const model=probe?.models.find(m=>m.model===models[id]);
    efforts[id]=selectedEffort(model,preferences.efforts[`${id}:${models[id]}`]);
    tiers[id]=selectedSpeed(model,preferences.tiers[`${id}:${models[id]}`]);
  }
  const probe=allProbes[agent] ?? null;
  const current=models[agent] ?? "";
  const dispatchModels=Object.fromEntries(Object.entries(models).map(([id,model])=>[id,registry.find(a=>a.id===id)?.available && (!allProbes[id]?.requires_auth || !!allProbes[id]?.auth) && !connecting[id] && !p.legacyConnecting[id] ? model : ""]));
  return <>{error && <div className="global-error" role="alert"><span>{error}</span><button className="outline-button" onClick={()=>void connect(agent)}>重试检查</button></div>}<ChatStage {...p} probe={probe} model={current} connecting={connecting[agent] ?? p.legacyConnecting[agent] ?? false} onConnect={()=>void connect(agent)} onModel={model=>save({...preferences,models:{...preferences.models,[agent]:model}})} collaborationModels={dispatchModels} collaborationEfforts={efforts} collaborationTiers={tiers} reasoningEffort={efforts[agent]} onReasoningEffort={value=>save({...preferences,efforts:{...preferences.efforts,[`${agent}:${current}`]:value}})} codexServiceTier={tiers[agent]} onServiceTier={value=>save({...preferences,tiers:{...preferences.tiers,[`${agent}:${current}`]:value}})} /></>;
}
