# Gemini / Antigravity 接入预研

日期：2026-11-22。

状态：仅完成官方文档阅读与接入方案记录，尚未实施或验证。用户已明确要求先不接入，后续再做。本文件不代表接入工作已获启动授权。

2026-11-22 实施补充：本次已选择官方 Gemini CLI ACP v1 并加入适配代码，未选择 Antigravity 路线。当前电脑没有 Gemini，未安装、执行或实机验收。详细入口、数据与权限限制见 [通用邀请与 Gemini 实现说明](verification/2026-11-22-generic-pairing.md)；下文 Antigravity 内容继续作为候选预研保留。

## 1. 结论与范围

从官方文档看，可以通过 Antigravity 为 AI-gent 增加使用 Gemini 的 Agent。Antigravity 提供 Agent 运行环境；Gemini 是底层模型。未来界面需区分成员身份、运行组件和所选模型，不能把三者混成同一个配置。

建议先评估 Antigravity CLI 的 ACP 路线。它使用子进程和 stdio JSON-RPC，与本项目现有本机运行组件的集成方式相近。这是基于通信结构的工程判断，不代表协议可直接复用，也不代表它优于 SDK 的实际运行结果。

本次不安装 CLI 或 SDK，不登录账号，不读取凭据，不发送模型请求，不增加成员入口，不修改执行代码。

```text
AI-gent 界面与群会话
         |
         +-- Codex 适配器 ------ Codex App Server
         +-- DeepSeek 适配器 --- DeepSeek Harness
         +-- 待实现适配器 ------ Antigravity CLI / ACP ------ Gemini
                                    |
                                    +-- 备选：Python SDK 后台进程
```

## 2. 官方文档已说明的内容

| 内容 | 文档依据 | 本项目状态 |
| --- | --- | --- |
| Antigravity SDK 是 Python Agent SDK，提供工具执行、上下文管理、策略和会话生命周期能力 | [SDK 概览](https://antigravity.google/docs/sdk/overview/) | 未安装、未运行 |
| SDK 快速开始使用 `google-antigravity` 包与 `GEMINI_API_KEY` | [SDK 快速开始](https://antigravity.google/docs/sdk/overview/#quickstart) | 未配置凭据 |
| SDK 文档提供 Google Cloud / Vertex 配置方式 | [SDK 概览中的企业配置](https://antigravity.google/docs/sdk/overview/) | 未验证账号、地区或项目要求 |
| `agy acp` 可启动 ACP 服务，客户端通过子进程 stdin / stdout 使用 JSON-RPC 通信 | [ACP 官方说明](https://antigravity.google/docs/cli/acp/) | 未实现客户端 |
| ACP 文档说明可使用 CLI 的工具、设置与缓存登录凭据 | [ACP 快速开始](https://antigravity.google/docs/cli/acp/#quick-start) | 未验证本机登录状态或可用范围 |
| 入门文档列出 Windows CLI 安装方式，以及 Python 3.10+ 的 SDK 起步要求 | [入门文档](https://antigravity.google/docs/getting-started?tab=sdk) | 未验证本机运行兼容性 |

文档中的安装命令、示例代码和接口名是调研资料。本次没有执行。后续实施前需重新确认官方文档、包来源、协议版本和支持平台，不以本次记录代替版本核对。

## 3. 两条候选路线

| 对比项 | CLI / ACP：优先评估 | Python SDK：备选 |
| --- | --- | --- |
| 进程结构 | Rust 管理 `agy acp` 子进程 | Rust 管理 Python 后台进程，后台调用 SDK |
| 通信 | 官方已说明 stdio JSON-RPC；具体方法及事件需核对 | 应用需自行定义 Rust 与 Python 间的消息格式 |
| 认证 | 文档说明可复用 CLI 缓存凭据 | 文档示例使用 API Key 或 Google Cloud 凭据 |
| 工具与权限 | 以 CLI 实际返回与 ACP 能力为准 | 以 SDK 配置、策略与实际工具为准 |
| 部署依赖 | CLI 及其对应版本和认证配置 | Python 环境、SDK 及其运行依赖 |
| 主要待确认项 | 握手、会话、模型配置、工具事件、审批、中断及 Windows 进程清理 | 流式 API、生命周期、权限回调、持久化及 Windows 部署 |

不把普通 `agy -p` 文本输出作为完整 Agent 协议的替代品。不能只根据最终回复文本推断工具执行成功、任务完成或取消成功。

## 4. 认证与额度边界

- SDK 示例中的 Gemini API Key 与个人聊天订阅不是同一个概念。不能承诺 SDK 会消费个人订阅额度。
- ACP 可复用 CLI 缓存凭据，不等于所有账号、地区、套餐和模型都可用。需要依据后续选定的认证路线确认权限及额度。
- 不提取或复制桌面客户端内部 token，不通过非公开接口绕过登录流程。
- 密钥不得写入项目源码、聊天消息、任务导出或普通日志。错误信息应脱敏。
- 登录成功、列出模型、实际执行任务、额度可用是不同结果，需分别记录。

## 5. 对当前项目的影响

本项目当前多处仍只接受 Codex / DeepSeek。增加第三位 Agent 需要完整梳理，不能只加一个按钮。

| 现有位置 | 后续工作 |
| --- | --- |
| `src/features/chats/api.ts` | 增加成员身份、探测信息与实际能力声明，减少双成员假设 |
| `src/features/chats/mentions.ts`、`GroupDialog.tsx`、`GroupMembers.tsx` | 支持新成员的点名、加入和移出；保持引用与附件不触发投递 |
| `src/Workbench.tsx`、`ChatStage.tsx`、`GroupMemberStatus.tsx` | 接入模型选择、认证状态、发送、中断和独立成员状态 |
| `src-tauri/src/groups.rs` | 更新成员白名单与人数限制，保持群设置对所有会话生效 |
| `src-tauri/src/collaboration.rs` | 清理固定双成员遍历、模型字段和交接判断；保持用户点名与最新指令优先 |
| `src-tauri/src/commands/mod.rs` | 增加运行组件接口，并保持会话归属、归档状态与执行准入校验 |
| `src-tauri/migrations/012_parallel_members.sql` 及后续迁移 | 现有成员约束仅允许两家；通过新迁移扩展，不能修改已发布迁移代替升级 |
| 运行时绑定与任务结果 | 新成员的会话 ID、轮次、版本、工作目录和事件应能追溯 |

现有会话隔离继续适用：只提供当前需求会话的上下文，不自动读取其他会话或私聊。群成员与项目目录仍共享。新成员接入不应顺带改变跨会话执行限制、自动提交或发布规则。

## 6. 后续实施前必须确认

以下均为待验证项，不是已支持承诺：

1. 固定 CLI 或 SDK 版本，核对 Windows 可执行文件、后台启动、中文目录及退出清理。
2. 核对 ACP 或 SDK 的真实握手、能力发现、模型选择和配置方式。模型名称与思考选项以实际可用结果为准。
3. 确认多轮会话、重启恢复、工作目录绑定与版本不兼容时的处理。
4. 确认文本流、思考摘要和工具事件的 schema、顺序与关联 ID；没有公开思考内容时不显示空栏目。
5. 确认审批请求、拒绝和取消的协议，以及中断后工具子进程是否仍运行。不支持的权限请求应明确拒绝。
6. 确认文件读取、写入和命令执行的真实权限边界；工作目录不能代替沙箱。
7. 确认认证、网络、额度不足、进程断开和模型不可用时的错误映射与草稿保留。
8. 确认群内三成员投递、单独点名、运行中插话、移出成员、会话归档和删除之间的一致性。

## 7. 后续实施顺序

| 阶段 | 工作 | 进入下一阶段的条件 |
| --- | --- | --- |
| 重新确认需求 | 用户明确要求开始接入，确认认证与执行范围 | 获得当次实施授权；本文件不自动触发后续工作 |
| 最小协议验证 | 先验证 ACP 的启动、握手、单轮消息、事件与停止 | 保存脱敏证据；确认关键能力可映射 |
| 路线选择 | ACP 可用则继续；不满足关键能力时评估 SDK | 写明取舍、依赖与已知限制 |
| 独立私聊 | 接入探测、模型、历史、审批和中断 | 单独使用时可以追踪终态与错误 |
| 群会话 | 扩展成员数据与调度，接入第三位成员 | 会话隔离和用户投递规则保持一致 |
| 用户验收与发布 | 按届时用户允许的范围做验证，列出未验证内容 | 用户确认后再决定提交、推送或发布 |

截至本次记录，未启动上述实施阶段。没有安装、模型调用、接入代码或运行验证结果。
