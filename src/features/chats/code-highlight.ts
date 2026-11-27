import { createHighlighterCore, type HighlighterCore, type LanguageRegistration } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

type LangLoader = () => Promise<{ default: LanguageRegistration | LanguageRegistration[] }>;

const LANG_LOADERS: Record<string, LangLoader> = {
  sql: () => import("shiki/langs/sql.mjs"),
  javascript: () => import("shiki/langs/javascript.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  typescript: () => import("shiki/langs/typescript.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  jsonc: () => import("shiki/langs/jsonc.mjs"),
  shellscript: () => import("shiki/langs/shellscript.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  scss: () => import("shiki/langs/scss.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  xml: () => import("shiki/langs/xml.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  java: () => import("shiki/langs/java.mjs"),
  c: () => import("shiki/langs/c.mjs"),
  cpp: () => import("shiki/langs/cpp.mjs"),
  csharp: () => import("shiki/langs/csharp.mjs"),
  php: () => import("shiki/langs/php.mjs"),
  ruby: () => import("shiki/langs/ruby.mjs"),
  swift: () => import("shiki/langs/swift.mjs"),
  kotlin: () => import("shiki/langs/kotlin.mjs"),
  docker: () => import("shiki/langs/docker.mjs"),
  diff: () => import("shiki/langs/diff.mjs"),
  graphql: () => import("shiki/langs/graphql.mjs"),
};

const LANG_ALIASES: Record<string, keyof typeof LANG_LOADERS> = {
  sql: "sql",
  mysql: "sql",
  postgres: "sql",
  postgresql: "sql",
  pgsql: "sql",
  sqlite: "sql",
  hive: "sql",
  hql: "sql",
  sparksql: "sql",
  presto: "sql",
  trino: "sql",
  clickhouse: "sql",
  plsql: "sql",
  tsql: "sql",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  javascript: "javascript",
  jsx: "jsx",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  typescript: "typescript",
  tsx: "tsx",
  py: "python",
  python: "python",
  python3: "python",
  rs: "rust",
  rust: "rust",
  json: "json",
  json5: "jsonc",
  jsonc: "jsonc",
  sh: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  shell: "shellscript",
  shellscript: "shellscript",
  console: "shellscript",
  yml: "yaml",
  yaml: "yaml",
  toml: "toml",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  md: "markdown",
  markdown: "markdown",
  xml: "xml",
  svg: "xml",
  go: "go",
  golang: "go",
  java: "java",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  "c++": "cpp",
  cs: "csharp",
  csharp: "csharp",
  "c#": "csharp",
  php: "php",
  rb: "ruby",
  ruby: "ruby",
  swift: "swift",
  kt: "kotlin",
  kotlin: "kotlin",
  dockerfile: "docker",
  docker: "docker",
  diff: "diff",
  patch: "diff",
  gql: "graphql",
  graphql: "graphql",
};

const MAX_HIGHLIGHT_CHARS = 80_000;
const MAX_CACHE_ENTRIES = 200;

let highlighterPromise: Promise<HighlighterCore> | null = null;
let highlighterInstance: HighlighterCore | null = null;
const loadedLangs = new Set<string>();
const pendingLangLoads = new Map<string, Promise<void>>();
const htmlCache = new Map<string, string>();

async function getHighlighter(): Promise<HighlighterCore> {
  if (highlighterInstance) return highlighterInstance;
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const [lightTheme, darkTheme] = await Promise.all([
        import("shiki/themes/github-light.mjs"),
        import("shiki/themes/github-dark.mjs"),
      ]);
      const instance = await createHighlighterCore({
        themes: [lightTheme.default, darkTheme.default],
        langs: [],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      });
      highlighterInstance = instance;
      return instance;
    })();
  }
  return highlighterPromise;
}

export function resolveHighlightLanguage(language?: string): string | null {
  if (!language) return null;
  const normalized = language.trim().toLowerCase();
  return LANG_ALIASES[normalized] ?? null;
}

async function ensureLanguageLoaded(highlighter: HighlighterCore, lang: string): Promise<void> {
  if (loadedLangs.has(lang)) return;
  const existing = pendingLangLoads.get(lang);
  if (existing) return existing;

  const loader = LANG_LOADERS[lang];
  if (!loader) return;

  const task = loader()
    .then(async mod => {
      await highlighter.loadLanguage(mod.default);
      loadedLangs.add(lang);
    })
    .finally(() => {
      pendingLangLoads.delete(lang);
    });
  pendingLangLoads.set(lang, task);
  return task;
}

function storeCachedHtml(cacheKey: string, html: string): void {
  if (htmlCache.size >= MAX_CACHE_ENTRIES) {
    const oldestKey = htmlCache.keys().next().value;
    if (oldestKey !== undefined) htmlCache.delete(oldestKey);
  }
  htmlCache.set(cacheKey, html);
}

/**
 * Synchronously returns highlighted HTML when cached or when the language
 * grammar is already loaded in memory, avoiding async DOM swaps during streaming.
 */
export function highlightCodeSync(code: string, language?: string): string | null {
  if (!code || code.length > MAX_HIGHLIGHT_CHARS) return null;
  const canonicalLang = resolveHighlightLanguage(language);
  if (!canonicalLang) return null;

  const cacheKey = `${canonicalLang}\0${code}`;
  const cached = htmlCache.get(cacheKey);
  if (cached !== undefined) return cached;

  if (!highlighterInstance || !loadedLangs.has(canonicalLang)) return null;

  try {
    const html = highlighterInstance.codeToHtml(code, {
      lang: canonicalLang,
      themes: {
        light: "github-light",
        dark: "github-dark",
      },
      defaultColor: false,
    });
    storeCachedHtml(cacheKey, html);
    return html;
  } catch {
    return null;
  }
}

export async function highlightCode(code: string, language?: string): Promise<string | null> {
  if (!code || code.length > MAX_HIGHLIGHT_CHARS) return null;
  const syncResult = highlightCodeSync(code, language);
  if (syncResult !== null) return syncResult;

  const canonicalLang = resolveHighlightLanguage(language);
  if (!canonicalLang) return null;

  const cacheKey = `${canonicalLang}\0${code}`;
  try {
    const highlighter = await getHighlighter();
    await ensureLanguageLoaded(highlighter, canonicalLang);
    if (!loadedLangs.has(canonicalLang)) return null;

    const cached = htmlCache.get(cacheKey);
    if (cached !== undefined) return cached;

    const html = highlighter.codeToHtml(code, {
      lang: canonicalLang,
      themes: {
        light: "github-light",
        dark: "github-dark",
      },
      defaultColor: false,
    });

    storeCachedHtml(cacheKey, html);
    return html;
  } catch {
    return null;
  }
}
