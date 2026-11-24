# AgentCrew

> 面向个人开发者的本地多 AI Agent 协作桌面工作台。

**AgentCrew** 基于 Tauri 2、Rust 与 React 构建，是一个**可扩展的本地多 AI Agent 协作桌面工作台**。通过统一的 Provider Adapter 架构、通用自助配对机制与标准协议（如 ACP），它可以将你本机安装的各类 AI 编程 Agent 汇聚到同一个桌面界面中。无需在多个终端和客户端之间来回切换，即可在一个工作台内完成独立私聊、多 Agent 群聊分工协作、本地项目代码执行、技能调用、任务结果归档与 Token 配额监控。

---

## 核心特性

### 1. 开放的多 Agent 接入与统一管理
- **可扩展的 Provider Adapter 架构**：所有 Agent 运行时通过统一的 `ProviderAdapter` 接口接入，不绑定单一厂商；接入新的 CLI / SDK / ACP Agent 只需实现对应适配器即可扩展。
- **通用自助配对（Self-Onboarding）**：点击左侧「接入」生成不绑定厂商的通用邀请提示词与 10 分钟有效配对码，发送给本机 Agent 即可通过回环 HTTP 自动识别身份、核验组件并完成接入；支持启动自动发现与并行探测、自动继承系统代理，并允许同一厂商创建多个独立实例。
- **统一模型配置浮层**：在输入框直接切换目标 Agent、模型、思考强度（Reasoning Effort）与速度档位（Service Tier）。

### 2. 多 Agent 群聊协作与需求会话
- **定向 `@` 与并行执行**：群内不加 `@` 时广播协商分工；`@Agent` 时定向投递；同时 `@` 多位成员时并行启动、互不阻塞。支持成员间相互 `@` 自动接力协作，以及执行中随时插话（持久排队优先处理）与一键停止。
- **群内多需求会话（Group Sessions）**：单个群聊支持创建、搜索、重命名、归档/恢复与删除多个独立会话；消息、摘要、任务与草稿按会话隔离，群成员和关联项目目录共享。
- **可控的上下文预算**：群内投递自动结合近期消息窗口、旧消息原文摘录与关键词历史检索，控制提示体积，避免长对话上下文无限膨胀。
- **丰富的交互体验**：支持消息引用回复、文本附件、图片上传与预览、Markdown/GFM 渲染及 `@` 高亮；工作台与会话侧边栏支持独立折叠与 `⌘B` / `Ctrl+B` 专注模式，并自动记忆布局偏好。

### 3. `/` 斜杠技能调用
- 在私聊或群聊输入框输入 `/` 即可检索并调用本机技能（单条消息最多组合 8 个技能）。
- 自动整合运行时原生技能（Codex `skills/list`、DeepSeek Harness SDK 技能目录）与本地技能目录（项目及用户目录下的 `.agents/skills`、`.codex/skills`、`.dsh/skills`，支持 `CODEX_HOME` / `DSH_HOME` 覆盖）。

### 4. 任务结果、工作日志与跨厂商复核
- 按任务自动记录执行轮次、来源消息、工具调用明细及 Git 工作区变更快照。
- 在「任务结果」面板可查看历史任务详情、导出 **Markdown / JSON**，并支持按需指派另一家 Agent 基于实际文件进行独立复核。

### 5. 配额与用量看板（Usage Dashboard）
- 本地统计各 Agent、模型以及私聊/群聊维度的 Token 消耗（输入、输出、缓存）与预估费用。
- 直观展示配额使用进度与重置倒计时，方便掌控多模型成本。

### 6. 本地优先与后台常驻
- **后台独立调度**：由 Rust 后台调度循环驱动队列与任务执行，窗口隐藏或前端刷新不中断协作；支持系统托盘常驻、托盘菜单一键暂停/继续/彻底退出，退出时自动清理管理的子进程组。
- **本地 SQLite 持久化与升级兼容**：全部会话、消息、任务结果与用量记录保存在本地 SQLite 数据库（当前 schema v18，版本升级前自动备份），不依赖云端同步，不读取或复制原厂登录密钥；本机 Agent 组件升级后自动向后兼容恢复历史会话并迁移版本元数据。

---

## Agent 适配与扩展性

AgentCrew 采用开放的适配器模式，不绑定特定厂商，也不托管或复制任何登录密钥，而是直接复用你本机已安装并登录的 Agent 运行组件。

### 当前内置适配

| Agent | 接入协议 / 运行组件 | 登录与配置方式 | 核心能力 |
| --- | --- | --- | --- |
| **Codex** | `Codex App Server`（自动发现客户端内置组件或 `PATH` 中的 `codex` CLI） | 本机执行 `codex login` 或使用 Codex 桌面端登录态 | 流式回复、工具调用、会话恢复、交互审批、思考深度/速度档位 |
| **DeepSeek** | `DeepSeek Harness SDK` | 在 DeepSeek Harness 中配置 API 凭证或环境变量 | 流式回复、默认工具集、私聊/群聊会话恢复、进程组取消 |
| **Gemini** | `Gemini CLI` 或 `Antigravity (agy)` 的 **ACP v1** 协议 | 本机完成 Gemini CLI / `agy` 登录配置 | 流式回复、工具事件、模型目录、会话恢复、交互审批 |

### 扩展更多 Agent

对于本机使用的其他 AI 编程 Agent（例如 Claude Code 或其他支持 CLI / SDK / ACP 协议的工具），只需在 [`src-tauri/src/providers.rs`](src-tauri/src/providers.rs) 中实现 `ProviderAdapter` trait 并注册到 `BUILT_IN_DRIVERS`，即可无缝复用私聊、群聊协作、技能调用、任务归档与用量统计等全部工作台能力。

---

## 快速开始

### 环境要求

- **Node.js** 22.12+ & **npm**
- **Rust** 1.90+（含 Cargo）
- **Tauri 2 系统依赖**：
  - **macOS**：Xcode Command Line Tools（macOS 12.0+）
  - **Windows**：MSVC C++ 构建工具链与 WebView2（详见 [Tauri 官方前置要求](https://v2.tauri.app/start/prerequisites/)）
- 至少安装并配置好一个上述支持的本地 Agent 运行组件

### 安装与开发运行

```bash
# 1. 安装前端依赖
npm ci

# 2. 启动桌面开发版（首次运行会自动编译 Rust 后端）
npm run desktop
```

> 说明：`npm run dev` 仅启动前端 Vite 预览，原生的目录选择、本地 SQLite 数据库及 Agent 进程管理需要通过 `npm run desktop` 在桌面环境中运行。

### 构建安装包

```bash
npm run desktop:build
```

打包脚本 [`scripts/package-app.mjs`](scripts/package-app.mjs) 会按当前操作系统自动构建并将产物输出至 `releases/` 目录：
- **macOS**：使用 [`src-tauri/tauri.macos.conf.json`](src-tauri/tauri.macos.conf.json) 构建 `AgentCrew.app` 与 `.dmg`
- **Windows**：构建 NSIS 安装包 `.exe`（构建前请先关闭正在运行的 AgentCrew 窗口）

### 常用研发命令

```bash
npm run build         # TypeScript 类型检查 + 前端生产构建
npm run check:rust    # Rust 后端编译检查
npm run test:rust     # 运行 Rust 单元测试（不消耗模型额度）
```

如需运行依赖本机真实运行组件的集成探针测试（注意：带实际对话的探针会消耗账号模型额度）：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_probe -- --ignored --nocapture
```

---

## 使用指南

1. **接入本机 Agent**
   - 首次启动会自动探测本机已安装的运行组件。
   - 也可点击左侧「接入」，复制通用邀请提示词发给同机 Agent，待其回调配对接口并完成核验后即可使用。
2. **绑定本地工作空间**
   - 独立私聊无需绑定项目即可直接对话；需要读写仓库代码时，在工作空间中选择本地目录，或在群聊「群设置」中关联/切换项目目录。
3. **在群聊中协作**
   - 创建群聊并选择多位 Agent 成员。
   - 在群内按需求建立独立会话；直接发送消息广播分工，或使用 `@成员` 指定单个/多个成员并行执行。
   - 输入 `/` 即可附加本机技能；执行期间可随时插话排队或点击停止取消任务。
4. **查看结果与用量**
   - 点击群聊右上角「任务结果」查看文件变更快照、工具调用日志、导出报告或发起另一家 Agent 复核。
   - 打开用量看板查看各模型 Token 消耗、预估费用与额度重置倒计时。

---

## 项目结构

```text
AgentCrew/
├── src/                               # React + TypeScript 前端界面
│   ├── Workbench.tsx                  # 主工作台布局、侧边栏与视图切换
│   ├── components/                    # 通用组件（Avatar、Dialog、Icon）
│   └── features/
│       ├── chats/                     # 聊天舞台、群会话、成员管理、@提及、模型配置、技能、任务日志与结果
│       ├── projects/                  # 本地项目工作空间 API
│       └── usage/                     # Token 用量与配额看板
├── src-tauri/                         # Tauri 2 + Rust 后端
│   ├── src/
│   │   ├── providers.rs               # 统一 ProviderAdapter 注册与能力抽象
│   │   ├── codex.rs                   # Codex App Server 运行时与会话管理
│   │   ├── deepseek.rs                # DeepSeek Harness SDK 运行时
│   │   ├── gemini.rs                  # Gemini CLI / agy ACP v1 运行时
│   │   ├── agents.rs / pairing.rs     # Agent 成员表与本机回环 HTTP 自助配对
│   │   ├── groups.rs / group_sessions.rs / group_context.rs
│   │   │                              # 群聊、需求会话与上下文预算管理
│   │   ├── collaboration.rs           # 群内调度队列、自动接力与任务生命周期
│   │   ├── skills.rs                  # 原生与本地目录技能发现与注入
│   │   ├── task_results.rs            # 任务归档、Git 变更快照与导出
│   │   ├── usage.rs                   # Token 用量记录与费用估算
│   │   ├── process.rs / lifecycle.rs  # 跨平台进程组管理、代理继承与托盘生命周期
│   │   ├── storage/                   # SQLite 存储层与自动迁移备份
│   │   └── commands/                  # Tauri IPC 命令入口
│   ├── migrations/                    # SQLite 增量迁移脚本（001 ~ 018）
│   ├── tauri.conf.json                # 通用 / Windows Tauri 配置
│   └── tauri.macos.conf.json          # macOS 打包配置
├── scripts/
│   └── package-app.mjs                # 跨平台打包与产物归集脚本
└── docs/                              # 产品需求、架构设计、协作协议与验收记录
```

---

## 路线图（Roadmap）

下一阶段将重点围绕**代码交付闭环**与**实时响应性能**持续演进：

- **Git Worktree 隔离与可视化 Diff / Commit / PR 闭环**
  - 支持按需求会话或任务自动创建独立的 Git Worktree，避免多会话、多 Agent 并行改动同一仓库时产生文件冲突或污染当前分支。
  - 提供内置的可视化文件变更 Diff 审阅视图，直接查看、筛选和核对 Agent 产生的代码修改。
  - 打通从变更审阅到代码落地的最后一公里，支持在工作台内一键 Commit、Push 以及创建 Pull Request。
- **基于 Tauri Event 的实时事件推送架构**
  - 将当前前端基于定时轮询的消息与状态同步，全面升级为由 Rust 后端主动驱动的 Tauri Event 实时事件流推送。
  - 显著提升流式输出与工具调用状态的跟手度，同时在空闲状态下几乎实现零轮询、零额外 CPU 开销。
- **更多 Agent 适配与体验增强**
  - 扩展更多主流本地编程 Agent（如 Claude Code）的开箱即用内置适配与自动发现。
  - 持续完善桌面端专注编码体验（如暗黑主题与更紧凑的高信息密度布局）。

---

## 文档导航

更多产品设计、架构细节与历史验收记录请参阅 `docs/` 目录：

| 文档 | 说明 |
| --- | --- |
| [产品需求](docs/01-产品需求.md) | 用户流程、核心范围与功能需求 |
| [技术设计](docs/02-技术设计.md) | 整体架构、本地存储、进程与权限边界 |
| [Agent 接入与协作协议](docs/03-Agent接入与协作协议.md) | 厂商接入协议、统一事件、状态机与交接机制 |
| [开发计划](docs/04-开发计划.md) | 技术验证与分阶段实施规划 |
| [验收清单](docs/05-验收清单.md) | 功能、失败恢复与发布检查项 |
| [界面设计](docs/06-界面设计.md) | 聊天工作台与交互设计说明 |
| [Agent 自助接入与持续协作方案](docs/08-Agent自助接入与持续协作改进方案.md) | 通用配对、动态成员实例与后台调度设计 |
| [PostgreSQL 多端数据共享方案](docs/09-PostgreSQL多端数据共享迁移方案.md) | 后续多端数据共享架构预研 |
| [验收记录目录](docs/verification/) | 各功能阶段的实测与验收日志 |

---

## 数据与安全说明

- **数据归属本地**：数据库、附件缓存与任务证据全部保存在本机 Tauri 应用数据目录中，项目源码始终保留在你选择的原始目录内。
- **凭证隔离**：AgentCrew 不读取、存储或转发任何厂商的 API Key 或登录凭据，认证完全交由对应 Agent 原生运行组件处理。
- **权限边界**：工作目录约束不等同于完整的操作系统级沙箱隔离；Agent 执行命令与文件写入行为受对应运行时策略控制，请在执行高风险操作前确认工作区状态并做好 Git 版本管理。
