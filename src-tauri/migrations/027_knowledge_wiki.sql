-- 1. 全局连接与上下文预算配置（单行表 id = 1）
CREATE TABLE IF NOT EXISTS wiki_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 1,
    base_url TEXT NOT NULL DEFAULT 'http://127.0.0.1:19828',
    api_token TEXT NOT NULL DEFAULT '',
    habits_budget_bytes INTEGER NOT NULL DEFAULT 1800,
    l0_map_budget_bytes INTEGER NOT NULL DEFAULT 800,
    read_budget_bytes INTEGER NOT NULL DEFAULT 6000,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT OR IGNORE INTO wiki_settings (id) VALUES (1);

-- 2. 知识源注册表（兼容 LLM Wiki 项目与本地 Markdown 大目录）
CREATE TABLE IF NOT EXISTS wiki_sources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    source_kind TEXT NOT NULL DEFAULT 'llm_wiki',
    tier TEXT NOT NULL DEFAULT 'core_wiki',
    root_path TEXT NOT NULL,
    content_subdir TEXT NOT NULL DEFAULT 'wiki',
    inbox_subdir TEXT NOT NULL DEFAULT '00-inbox',
    doc_count INTEGER NOT NULL DEFAULT 0,
    index_state TEXT NOT NULL DEFAULT 'idle',
    last_indexed_at TEXT
);

-- 3. 全局常驻「个人通用习惯」勾选页
CREATE TABLE IF NOT EXISTS wiki_habit_pages (
    source_id TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    title TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source_id, rel_path)
);

-- 4. 文档元数据与增量同步游标表（基于 mtime_ms + size_bytes 毫秒比对）
CREATE TABLE IF NOT EXISTS wiki_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    top_domain TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '',
    is_deprecated INTEGER NOT NULL DEFAULT 0,
    mtime_ms INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    UNIQUE (source_id, rel_path)
);
CREATE INDEX IF NOT EXISTS idx_wiki_docs_domain ON wiki_documents(source_id, is_deprecated, top_domain);

-- 5. 章节级 FTS5 全文倒排索引表（BM25 相关性排序）
CREATE VIRTUAL TABLE IF NOT EXISTS wiki_sections_fts USING fts5(
    source_id UNINDEXED,
    rel_path UNINDEXED,
    section_heading,
    body,
    tokenize = 'unicode61'
);

-- 6. 双链关系边表（支撑一跳 [[关联规则]] 提示与跳转）
CREATE TABLE IF NOT EXISTS wiki_links (
    source_id TEXT NOT NULL,
    from_path TEXT NOT NULL,
    to_target TEXT NOT NULL,
    PRIMARY KEY (source_id, from_path, to_target)
);

-- 7. 会话 / 群聊作用域绑定表
CREATE TABLE IF NOT EXISTS conversation_wiki_bindings (
    conversation_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    domain_filter TEXT,
    include_archives INTEGER NOT NULL DEFAULT 0,
    last_injected_hash TEXT
);

PRAGMA user_version = 27;
