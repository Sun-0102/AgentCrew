# PostgreSQL 多端数据共享迁移方案

## 1. 背景与目标

当前 AgentCrew / AI-gent 桌面端采用本地内嵌 SQLite（`rusqlite` + `bundled`）作为持久化存储，数据库文件位于各设备的本地应用数据目录（macOS 下为 `~/Library/Application Support/local.ai-gent.desktop.dev/ai-gent.sqlite`）。

为了实现**多端共享数据**（在多台电脑之间同步项目、对话、群组、成员配置及任务记录），需要将存储层从单机 SQLite 迁移至 PostgreSQL（以下简称 PG），并同步解决桌面端特有的路径映射、进程运行态隔离与附件跨端访问问题。

---

## 2. 当前存储架构现状

在 `src-tauri/` 中，数据层现状如下：

- **依赖与驱动**：`src-tauri/Cargo.toml` 使用 `rusqlite = { version = "0.38", features = ["bundled"] }`，为同步阻塞式连接。
- **连接与迁移入口**：`src-tauri/src/lib.rs` 在启动阶段调用 `storage::Store::open(data_dir.join("ai-gent.sqlite"))`，随后依次执行 `images::recover`、`codex::recover`、`deepseek::recover`、`gemini::recover`、`collaboration::recover`、`pairing::recover`。
- **迁移脚本**：`src-tauri/migrations/` 下共有 16 个 SQL 文件（`001_projects.sql` ~ `016_generic_pairing_gemini.sql`），由 `src-tauri/src/storage/mod.rs` 通过 `PRAGMA user_version` 顺序执行，并在旧版本（`1..=15`）升级前使用 `VACUUM INTO` 自动备份。
- **代码耦合范围**：目前没有抽象统一的 ORM 或 Repository Trait，共有 21 个 Rust 源文件（`storage/mod.rs`、`codex.rs`、`deepseek.rs`、`gemini.rs`、`groups.rs`、`collaboration.rs`、`agents.rs` 等）直接依赖 `Store` 与 `rusqlite`，且 SQL 中包含 `PRAGMA`、`VACUUM INTO`、`?1, ?2` 位置占位符等 SQLite 专属语法。

---

## 3. 多端共享必须解决的 4 个核心问题

桌面端 Agent 工具与纯 Web 应用不同，不能只做“换个数据库连接串”，还必须处理以下 4 类单机强耦合问题：

### 3.1 本地项目路径跨设备映射
- **现状**：`projects.primary_root` 存储的是本机绝对路径（例如 macOS 的 `/Users/xxx/repo` 或 Windows 的 `D:\repo`）。
- **问题**：A 电脑创建的项目路径在 B 电脑上不存在，直接校验目录会报错。
- **方案**：
  - 云端 `projects` 表仅保存逻辑项目信息（`id`、`name`、可选的 `repo_remote_url` 等）。
  - 在每台设备的本地配置（或新增设备-路径绑定表 `project_device_roots(project_id, device_id, local_root)`）中维护本机磁盘路径映射；未绑定时提示用户在当前设备选择对应本地目录。

### 3.2 进程运行态与 `recover` 隔离
- **现状**：启动时 `recover` 会扫描未结束的任务/会话/配对记录并清理或重置异常进程状态。
- **问题**：多台设备共用同一个 PG 库时，如果 B 电脑启动 `recover`，可能误把 A 电脑正在运行的任务判定为异常中断并重置。
- **方案**：
  - 为每台客户端生成并持久化一个本机唯一 `device_id`（存放在本机 `app_data_dir`）。
  - 在运行态相关表（任务运行记录、进程 PID、配对状态等）增加 `device_id` 字段，各端 `recover` 仅处理 `device_id = 本机` 的记录。

### 3.3 图片与产物附件跨端读取
- **现状**：截图、上传图片及任务产物文件保存在本机磁盘路径，数据库仅记录路径或元数据。
- **问题**：另一台设备无法读取原设备磁盘上的文件路径。
- **方案**：
  - **轻量方案（推荐起步阶段）**：在 PG 中新增附件二进制表（`BYTEA` + `sha256` + `mime_type`），客户端按需拉取并缓存到本机 `app_data_dir`。
  - **扩展方案**：后续附件体积增大时，可切换为对象存储（MinIO / S3 / OSS），PG 仅存对象 Key。

### 3.4 网络连接与鉴权安全
- **个人多设备自用（推荐起步）**：通过 Tailscale / ZeroTier 组建虚拟局域网，客户端直连常开主机或 NAS 上的 PG（无需暴露公网 5432 端口，也无需额外开发后端服务）。
- **团队/对外分发**：切勿将 PG 账号密码直接打包在客户端中连接公网数据库；应在 PG 前增加一层轻量的 HTTP/WebSocket API 服务（可复用 Rust `axum`），客户端通过 Token 鉴权访问。

---

## 4. 分阶段实施路线

### 阶段 1：数据访问层迁至 PG（本机 PG 验证）
1. **引入 PG 驱动**：在 `src-tauri/Cargo.toml` 中引入 `sqlx`（开启 `postgres`、`runtime-tokio`、`migrate` 等特性）或 `deadpool-postgres` + `tokio-postgres`。
2. **改造 `Store` 结构**：
   - 将 `Store` 内部的 `PathBuf` + `Connection::open` 替换为 `PgPool` 连接池。
   - 由于 `tauri` 命令和部分调用为异步或 `spawn_blocking`，统一规划异步 `async` 接口或通过共享 Tokio Runtime 执行查询。
3. **重写迁移脚本（001 ~ 016）**：
   - 建立 `src-tauri/migrations_pg/` 目录，将原 16 个 `.sql` 脚本转换为 PG 兼容语法：
     - 移除 `PRAGMA foreign_keys`、`PRAGMA user_version`、`VACUUM INTO`；
     - 占位符从 `?1, ?2` 改为 `$1, $2`；
     - 检查自增主键、时间类型、布尔值及 `ON CONFLICT` 写法；
     - 使用 `sqlx::migrate!("./migrations_pg")` 管理版本。
4. **连接配置接入**：
   - 支持通过环境变量（如 `AGENTCREW_DATABASE_URL`）或本机配置文件读取连接串，例如：
     `postgresql://<user>:<password>@127.0.0.1:5432/agentcrew`
   - 在本地新建专用库（如 `agentcrew`）验证全部迁移脚本与核心读写流程。

### 阶段 2：补齐多端字段与路径适配
1. 本机生成并持久化 `device_id`。
2. 为运行态表补充 `device_id` 列，改造各模块 `recover(&store, &device_id)` 逻辑。
3. 改造项目目录绑定逻辑，支持不同设备绑定各自的 `local_root`。
4. 改造图片/产物存储，支持从 PG `BYTEA`（或对象存储）拉取并缓存到本机。

### 阶段 3：跨设备联调（全新初始化）
1. **不迁移历史 SQLite 数据**：直接在新库 `agentcrew` 中通过迁移脚本初始化空表结构，各端以全新数据起步。
2. 在两台设备（如双 Mac 或 Mac + Windows）上通过内网/Tailscale 连接同一 PG 实例，验证：
   - A 端新建群组/对话，B 端重启或刷新可见；
   - A 端正在执行任务时，B 端启动不会误触发 `recover` 重置；
   - 两端绑定各自本地代码目录后可正常协作。

---

## 5. 本地 PG 环境检查记录

- 本机已安装并运行 `PostgreSQL 18.6 (Homebrew)`，监听 `127.0.0.1:5432`。
- 使用 Homebrew 默认超级用户（当前系统用户名）可通过 TCP 正常连接与执行 SQL；后续正式开发建议在 PG 中创建独立数据库 `agentcrew` 与专用账号进行隔离。
