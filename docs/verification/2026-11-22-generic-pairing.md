# 通用邀请与 Gemini 执行实现说明

日期：2026-11-22。分支：`codex/agent-self-onboarding`。

状态：源码已加入实现；前端构建和 Rust 编译检查通过。未运行测试、模型探针或界面验收。此电脑没有 Gemini，本次未安装、启动 Gemini 或配置其凭证。没有提交或推送，也没有重启现有 AI-gent 窗口或打开用户数据库。

## 本次改动

| 项目 | 实现 |
| --- | --- |
| 通用邀请 | 添加弹窗移除厂商下拉框，只需名称；生成 v2 邀请，等待 Agent 自己识别并上报 |
| 自报与核验 | 请求包含 provider_kind、runtime_kind 和可选运行线索；界面分别显示自报身份和实际核验身份、版本 |
| 协议兼容 | 新增 `/local/v2/health`、`/local/v2/pairing-guide`、`/local/v2/pairings/claim` 与结果查询；v1 路径保留，旧邀请仍校验厂商与协议号 |
| Gemini 执行 | 仅实现官方 Gemini CLI 的 ACP v1 路线，由 Gemini CLI 自己执行；不调用 Codex 代办，不适配 Antigravity |
| 执行交互 | 按成员绑定会话，处理消息与工具事件、单次审批、取消、真实终态与中断状态；接入时不发送 `session/prompt` |
| 旧数据 | 新增 schema 16 迁移；保留现有成员、ID、会话、群成员关系、消息、任务和原模型配置 |

只持有邀请码或收到 HTTP 202 不表示已接入。应用检查完成并保存 active 后才报告接入成功；接入成功也不表示模型任务已完成。未知厂商、未适配的组件组合在领取前返回错误，不消耗有效邀请码。

## 数据与历史

新增 `016_generic_pairing_gemini.sql`，不改写已运行的旧迁移。

- `pairing_invitations.protocol_version` 的旧记录默认值为 1，保留原 expected_provider。新 v2 邀请使用协议号 2，expected_provider 留空，不参与厂商匹配。
- `pairing_claims` 增加自报厂商/组件与核验厂商/组件/版本。核验字段和成员激活在同一事务内保存。
- 新增 `gemini_sessions`，实际 Gemini 会话映射仍通过既有 `agent_sessions` 关联到成员与逻辑会话。消息进入原消息表，并保留实际 agent_id。
- Gemini 的队列继续复用 `group_task_inputs` 和 `agent_attempts`，不增加第二套任务系统。归档、项目目录变更和会话删除处理新增运行绑定。
- 不自动创建 Gemini 成员，不移除或要求重新配对既有 Codex、DeepSeek。新增成员是独立身份，只有用户选择后才加入群。

本次未执行数据库迁移。新版首次启动时，沿用现有备份和事务迁移机制升级到 schema 16。

## Gemini 路线与实际限制

适配器从本机已有官方 npm 包 `@google/gemini-cli` 的安装元数据定位入口，并用已有 Node.js 启动。只使用应用独立发现的组件，不执行 HTTP 请求提供的路径、脚本或任意参数。自定义安装位置可由用户在本地配置 `AI_GENT_GEMINI_PACKAGE` 为包目录；这不是 HTTP 配置项。

固定运行入口为 `node <已核验入口> --acp --approval-mode default`。无模型版本检查和 initialize 核验实际 Gemini CLI 身份、版本、ACP v1 与会话恢复能力。模型目录取自该运行组件的会话元数据，不编造模型列表。检查时可建立无任务的元数据会话，不发模型提示词；不会调用 authenticate 更改原厂登录配置。

真实任务使用 session/new 或精确的 session/load，并按返回目录选择模型，再发送 session/prompt。只有实际 prompt 响应的 end_turn 才显示本轮完成；取消必须收到 cancelled 才确认取消。断线、未知终态、取消未确认会显示相应错误，不重放未知副作用，也不切换厂商或模型。

工具审批只支持原厂给出的 allow_once。其他客户端文件和终端代理能力未开放，未支持请求拒绝处理；Gemini 使用自己的原生工具。工作目录不是额外的系统沙箱，应用不宣称 Gemini 有 Codex 的 workspace-write 隔离。Gemini 的 `/` 入口目前读取本地 `.gemini/skills` 和共用 `.agents/skills`；不宣称已经覆盖原厂扩展提供的全部技能。

此实现针对已核对的 Gemini CLI ACP v1 合同。缺失组件、登录、协议能力或模型目录时明确失败，不作为可执行成员激活。没有本机 Gemini，因此实际版本兼容、登录、文件工具、审批、取消、恢复及模型执行均未验收；Mac 也未验收。

## 用户后续验收

以下步骤本次均未执行。真实私聊或群协作会使用原厂模型额度。

1. 保留现有成员；编译并重新启动新版后，核对旧聊天和群关系，以及 schema 16 升级备份。
2. 生成通用邀请，分别粘贴给本机 Codex 和 DeepSeek。每位使用新的一次性邀请。确认无需选择类型，应用显示自报与核验结果。
3. 对新成员分别私聊，核对独立会话、模型、消息、审批和取消；再按需加入测试群。不要为了试新邀请先移除旧成员。
4. 核对旧 v1 邀请仍按原厂商约束处理；新 v2 邀请不能通过 v1 领取。重复相同请求只产生同一个成员和 claim。
5. 当前电脑没有 Gemini，无需为本次验收安装。将 Gemini 实机验收标为“未安装，未执行”，不能用 Codex 的结果替代。
6. 将来用户自行安装并登录兼容 Gemini CLI 后，再检查 Gemini 自己上报接入、私聊、群内交接、单次审批、取消、恢复和结果导出。核对实际运行组件、原厂会话和任务尝试均来自 Gemini。

已执行的仅为 `npm run build` 与 `cargo check --manifest-path src-tauri/Cargo.toml`，均通过；这不代表迁移、配对或功能测试通过。当前运行中的旧窗口不会因源码修改自动成为新版。

接口依据：[Gemini CLI ACP 官方说明](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md)、[Gemini ACP 分派实现](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/acp/acpRpcDispatcher.ts)、[Gemini 会话管理实现](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/acp/acpSessionManager.ts)、[ACP v1 会话建立](https://agentclientprotocol.com/protocol/v1/session-setup)。源码接口会变化，实际组件握手不兼容时应停止并报告。
