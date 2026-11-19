# 思考模式选择

日期：2026-11-19。

后续交互更新：思考强度下拉框已替换为白色圆角弹出面板和分档滑杆，采用原生 Popover 与 range 控件，无新增依赖。拖拽按当前模型支持的档位吸附，标题即时更新；支持方向键、Home/End、恢复推荐档位、Escape 和点击外部关闭。速度闪电保留，与思考设置独立。

本次前端构建及 7 项测试通过。浏览器测试数据下真实鼠标拖拽验证 Codex 最低/最高映射 low/xhigh，方向键选择 medium；刷新保留、推荐重置、模型档位数量更新均通过。DeepSeek 验证 off/max，并确认发送 max；Codex 发送 medium 时仍保留 priority。390×720 弹层完全位于视口内，任务运行时关闭并禁用选择。脚本和截图在 `artifacts/reasoning-slider/`。没有重复模型调用或原生桌面完整点击验收。下文保留后台接入与首次验证记录。

- 输入框的模型选择旁新增思考模式选择。Codex 使用 `model/list` 中每个模型的 `supportedReasoningEfforts` 和 `defaultReasoningEffort`；不写死所有模型共用的档位。
- DeepSeek 从本机 Harness 0.2.1-rc.2 的公开 `DeepSeekAdapter.resolveModel` 读取能力，当前为 `off/low/high/max`。本机 SDK 源码确认 `initialize.reasoningEffort` 传入新建及恢复会话的 Agent 选项。参考：[官方 SDK 源码](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/server/src/server.ts)、[官方能力说明](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/providers.md)。
- 初次显示模型提供的推荐档位，用户可手动修改；之后按 Agent 和模型记住选择。模型切换时仅保留该模型支持的值，无可调能力时禁用。执行期间禁用模型和思考模式选择。
- 私聊命令传 `reasoningEffort`，群聊传 `codexReasoningEffort/deepseekReasoningEffort`。SQLite schema 9 按会话、Agent、模型保留档位，群内后续调度也读取该配置。升级前备份 schema 8 数据库。
- Codex 每轮 `turn/start.effort` 显式传值；DeepSeek 每次启动 SDK 时 `initialize.reasoningEffort` 显式传值，恢复同一会话也生效。旧调用未传值时保持兼容。后端检查所选值是否由该模型支持。

## 验证

| 检查 | 结果 |
| --- | --- |
| TypeScript、Vite、Rust 桌面调试构建 | 通过 |
| Rust 普通测试 | 17 通过，8 项真实运行测试默认忽略 |
| 前端测试 | 6 通过，含关闭思考、无效旧档位、无能力模型的回退检查 |
| Codex 真实调用 | 同一会话先 `low` 后 `ultra`，两轮均返回 REASONING_OK |
| DeepSeek 真实调用 | 同一会话先 `off` 后 `max`，两轮均返回 REASONING_OK |
| 浏览器交互 | 测试数据下确认私聊和群聊命令携带各自档位；切模型后可用档位更新；刷新保留 Codex high、DeepSeek off；运行中禁用选择 |
| 窄窗口 | 900×640、390×720 均无页面横向溢出，选择框在视口内 |

真实调用只在临时数据库及工作目录运行，无工具任务。命令：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml reasoning_selection -- --ignored --nocapture --test-threads=1
node --test tests/*.test.mjs
```

浏览器脚本及截图位于 `artifacts/reasoning/`，截图中的模型名称是 UI 测试数据。未进行原生桌面完整点击验收，未逐档调用全部模型。真实调用确认运行组件接受所选参数，不用于比较模型思考质量。
