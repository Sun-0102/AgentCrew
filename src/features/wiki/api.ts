import { invoke, isTauri } from "@tauri-apps/api/core";

export const WIKI_BINDING_NONE = "__none__";

export interface WikiSettings {
  enabled: boolean;
  base_url: string;
  api_token: string;
  habits_budget_bytes: number;
  l0_map_budget_bytes: number;
  read_budget_bytes: number;
  default_private_source_id?: string | null;
  updated_at: string;
}

export interface WikiDomainCount {
  domain: string;
  count: number;
}

export interface WikiSource {
  id: string;
  name: string;
  source_kind: "llm_wiki" | "local_folder" | string;
  tier: "core_wiki" | "archive" | string;
  root_path: string;
  content_subdir: string;
  inbox_subdir: string;
  doc_count: number;
  index_state: "idle" | "indexing" | "ready" | "error" | string;
  last_indexed_at: string | null;
  domains: WikiDomainCount[];
}

export interface WikiHabitPage {
  source_id: string;
  source_name: string;
  rel_path: string;
  title: string;
  enabled: boolean;
  sort_order: number;
  size_bytes: number;
}

export interface WikiBinding {
  conversation_id: string;
  source_id: string;
  domain_filter: string | null;
  include_archives: boolean;
  last_injected_hash: string | null;
}

export interface WikiSearchHit {
  source_id: string;
  source_name: string;
  rel_path: string;
  abs_path: string;
  top_domain: string;
  title: string;
  section_heading: string;
  snippet: string;
  tags: string;
  is_deprecated: boolean;
}

export interface WikiLinkRef {
  target: string;
  resolved_path: string | null;
  resolved_title: string | null;
}

export interface WikiReadResult {
  source_id: string;
  source_name: string;
  rel_path: string;
  abs_path: string;
  title: string;
  section: string | null;
  content: string;
  truncated: boolean;
  links: WikiLinkRef[];
}

export interface ExplicitWikiRef {
  source_id: string;
  rel_path: string;
  section?: string | null;
  title?: string;
}

export interface RemoteWikiProject {
  id: string;
  name: string;
  root_path: string;
  content_subdir: string;
}

export interface WikiConnectionTest {
  ok: boolean;
  base_url: string;
  message: string;
  version: string | null;
  projects: RemoteWikiProject[];
}

export interface IndexStats {
  source_id: string;
  scanned: number;
  indexed: number;
  removed: number;
  active_docs: number;
}

export interface ArchiveNoteResult {
  source_id: string;
  rel_path: string;
  abs_path: string;
  rescan_triggered: boolean;
}

const desktop = isTauri();

const defaultSettings: WikiSettings = {
  enabled: true,
  base_url: "http://127.0.0.1:19828",
  api_token: "",
  habits_budget_bytes: 1800,
  l0_map_budget_bytes: 800,
  read_budget_bytes: 6000,
  default_private_source_id: null,
  updated_at: "",
};

export async function getWikiSettings(): Promise<WikiSettings> {
  if (!desktop) return defaultSettings;
  return invoke<WikiSettings>("wiki_settings_get");
}

export async function saveWikiSettings(settings: WikiSettings): Promise<WikiSettings> {
  if (!desktop) return settings;
  return invoke<WikiSettings>("wiki_settings_save", { settings });
}

export async function testWikiConnection(baseUrl: string, apiToken: string): Promise<WikiConnectionTest> {
  if (!desktop) {
    return { ok: false, base_url: baseUrl, message: "浏览器预览模式不支持本机连接测试。", version: null, projects: [] };
  }
  return invoke<WikiConnectionTest>("wiki_test_connection", { baseUrl, apiToken });
}

export async function syncWikiProjects(): Promise<WikiSource[]> {
  if (!desktop) return [];
  return invoke<WikiSource[]>("wiki_sync_projects");
}

export async function listWikiSources(): Promise<WikiSource[]> {
  if (!desktop) return [];
  return invoke<WikiSource[]>("wiki_source_list");
}

export async function addLocalWikiSource(params: {
  rootPath: string;
  name?: string | null;
  tier?: "core_wiki" | "archive";
  contentSubdir?: string | null;
}): Promise<WikiSource> {
  return invoke<WikiSource>("wiki_source_add_local", {
    rootPath: params.rootPath,
    name: params.name ?? null,
    tier: params.tier ?? "core_wiki",
    contentSubdir: params.contentSubdir ?? null,
  });
}

export async function removeWikiSource(sourceId: string): Promise<void> {
  if (!desktop) return;
  await invoke("wiki_source_remove", { sourceId });
}

export async function reindexWikiSource(sourceId: string, force = false): Promise<IndexStats> {
  return invoke<IndexStats>("wiki_source_reindex", { sourceId, force });
}

export async function listWikiHabits(): Promise<WikiHabitPage[]> {
  if (!desktop) return [];
  return invoke<WikiHabitPage[]>("wiki_habit_list");
}

export async function setWikiHabit(params: {
  sourceId: string;
  relPath: string;
  title: string;
  enabled: boolean;
}): Promise<WikiHabitPage[]> {
  if (!desktop) return [];
  return invoke<WikiHabitPage[]>("wiki_habit_set", {
    sourceId: params.sourceId,
    relPath: params.relPath,
    title: params.title,
    enabled: params.enabled,
  });
}

export async function getWikiBinding(conversationId: string): Promise<WikiBinding | null> {
  if (!desktop) return null;
  return invoke<WikiBinding | null>("wiki_binding_get", { conversationId });
}

export async function setWikiBinding(params: {
  conversationId: string;
  sourceId: string | null;
  domainFilter?: string | null;
  includeArchives?: boolean;
}): Promise<WikiBinding | null> {
  if (!desktop) return null;
  return invoke<WikiBinding | null>("wiki_binding_set", {
    conversationId: params.conversationId,
    sourceId: params.sourceId,
    domainFilter: params.domainFilter ?? null,
    includeArchives: params.includeArchives ?? false,
  });
}

export async function searchWiki(params: {
  query: string;
  sourceId?: string | null;
  domainFilter?: string | null;
  includeArchives?: boolean;
  limit?: number;
  offset?: number;
}): Promise<WikiSearchHit[]> {
  if (!desktop) return [];
  return invoke<WikiSearchHit[]>("wiki_search", {
    query: params.query,
    sourceId: params.sourceId ?? null,
    domainFilter: params.domainFilter ?? null,
    includeArchives: params.includeArchives ?? false,
    limit: params.limit ?? 20,
    offset: params.offset ?? 0,
  });
}

export async function readWikiDoc(params: {
  sourceId?: string | null;
  relPath: string;
  section?: string | null;
}): Promise<WikiReadResult> {
  return invoke<WikiReadResult>("wiki_read_doc", {
    sourceId: params.sourceId ?? null,
    relPath: params.relPath,
    section: params.section ?? null,
  });
}

export async function archiveWikiNote(params: {
  sourceId: string;
  title: string;
  content: string;
  tags?: string[];
  sourceConversation?: string | null;
}): Promise<ArchiveNoteResult> {
  return invoke<ArchiveNoteResult>("wiki_archive_note", {
    sourceId: params.sourceId,
    title: params.title,
    content: params.content,
    tags: params.tags ?? [],
    sourceConversation: params.sourceConversation ?? null,
  });
}
