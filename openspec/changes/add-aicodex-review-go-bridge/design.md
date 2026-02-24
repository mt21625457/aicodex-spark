## Context

现有 webhook 自动审核实现位于 `crates/remote`，流程强依赖 GitHub App
安装信息、GitHub API token 与 GitHub payload 结构。目标是增加 Gitea
接入，同时满足以下硬约束：

- 第一阶段不改动 Vibe-Kanban 现有代码。
- 通过现有公开 HTTP API 与 Vibe-Kanban 交互。
- 新能力集中在 `aicodex-review/`，以独立服务部署与演进。

关键现实约束：

- 需要由 bridge 自行处理 webhook 签名和幂等。
- 由于不修改 Vibe-Kanban，bridge 不能依赖新增内部接口。
- 审核结果回写（评论）需由 bridge 直接调用 Gitea API 完成。

## Goals / Non-Goals

**Goals:**

- 在 `aicodex-review/` 构建可生产化 Go bridge 服务，接收 Gitea webhook。
- 通过 Vibe-Kanban 现有 HTTP API 触发并跟踪 review。
- 将 review 结果以 PR 评论形式回写到 Gitea。
- 提供可扩展工作流路由，支持后续“非 review”自动化能力接入。
- 全链路具备幂等、重试、可观测、可追踪。

**Non-Goals:**

- 不修改 `vibe-kanban` 原有 Rust/前端代码。
- 不重构现有 GitHub App 实现。
- 不在第一阶段实现复杂多租户权限中心（先使用单实例配置与项目级控制）。
- 不在第一阶段实现跨实例高可用调度（先单实例+持久化存储）。

## Decisions

### 1) 架构形态：独立 Go 服务（选择）

**Decision**

采用独立服务 `aicodex-review/`，不将 Go 代码嵌入 Rust crate。

**Rationale**

- 与“第一阶段不改动原系统”目标一致。
- Go 与 Gitea SDK 生态兼容度高，接入与维护成本更低。
- 可独立发布、回滚、扩缩容，避免影响主业务 API。

**Alternatives**

- 在 `crates/remote` 内新增 Gitea Rust 模块：可行，但与“不改主代码”冲突。
- 继续沿用 GitHub 逻辑并做 webhook 转换：适配复杂且可维护性差。

### 2) 与 Vibe-Kanban 集成方式：仅用公开 HTTP API（选择）

**Decision**

Bridge 使用已有 API 流程：

1. `POST /v1/review/init`
2. 上传 tar.gz 到 `upload_url`
3. `POST /v1/review/start`
4. `GET /v1/review/{id}/status`
5. 生成 review URL：`{VIBE_BASE_URL}/review/{id}`

**Rationale**

- 无需修改 Vibe-Kanban 代码或新增内部接口。
- 与现有 CLI 的行为一致，降低协议不确定性。

**Alternatives**

- 调用 `/v1/debug/pr-review/trigger`：依赖受保护调试接口，不适合作为标准集成。
- 直接调用 review worker：绕过主服务能力边界，不利于稳定性与兼容性。

### 3) 执行模型：Webhook 入队 + Worker 异步处理（选择）

**Decision**

HTTP webhook 线程只做校验、幂等判断、入队；重操作（clone/tar/upload/start/poll）
在 worker 中异步执行。

**Rationale**

- 减少 webhook 超时与重试风暴。
- 更易实现失败重试、限流、并发隔离。

**Alternatives**

- 同步执行全流程：实现简单，但超时风险高且不稳定。

### 4) 幂等策略：基于 `X-Gitea-Delivery`（选择）

**Decision**

使用持久化表（SQLite/PostgreSQL）记录 delivery id，建立唯一索引
`(provider, delivery_id)`。

**Rationale**

- 对 Gitea 重投递天然友好。
- 重启后仍可去重，避免重复触发 review。

**Alternatives**

- 内存去重：重启丢失，不满足生产稳定性。

### 5) 结果回写：Bridge 主动评论（选择）

**Decision**

Bridge 在状态变化时调用 Gitea SDK 在 PR 下评论，不依赖 Vibe 回调触发评论。

**Rationale**

- 在不改 Vibe 代码前提下，仍能闭环“审核 -> 回写”。
- 更可控，可统一评论模板与重试策略。

**Alternatives**

- 依赖 Vibe 的 webhook-review 成功/失败评论路径：当前为 GitHub 专用。

### 6) 可扩展路由：Workflow Registry（选择）

**Decision**

引入配置驱动的 workflow 路由：

- 输入：事件类型 + 仓库匹配 + 分支策略 + 指令（如评论命令）。
- 输出：workflow id（`review` / future workflows）。

第一阶段仅实现 `review`，保留接口以支持“其他需求”。

**Rationale**

- 避免后续每加一个自动化需求都重写 webhook 分发逻辑。

## Risks / Trade-offs

- [Risk] Vibe API 限流或短时不可用导致触发失败
  → Mitigation: 指数退避重试 + 最大重试次数 + 死信队列 + 告警。
- [Risk] 大仓库 clone/压缩耗时长
  → Mitigation: 并发限制、超时、仓库大小阈值、分阶段日志。
- [Risk] 评论重复刷屏
  → Mitigation: 评论幂等键（job_id）与“更新同一条评论”策略（后续可迭代）。
- [Risk] Token 泄露或权限过大
  → Mitigation: 最小权限 token、密钥管理、日志脱敏、定期轮换。
- [Risk] “不改主服务”限制导致能力边界不足
  → Mitigation: 在设计中预留 `v2` 内部 API 集成点，后续按需演进。

## Migration Plan

1. 创建 `aicodex-review/` Go 模块骨架与配置加载。
2. 实现 webhook ingress（签名、幂等、入队）。
3. 实现 review worker（clone/tar/vibe-api/poll）。
4. 实现 Gitea 评论回写与失败重试。
5. 接入日志、指标、追踪（request_id, delivery_id, review_id, job_id）。
6. 在测试 Gitea 仓库灰度启用（单仓库 -> 多仓库）。
7. 稳定后扩展 workflow registry 的第二个 workflow（可选）。

回滚策略：

- 直接下线 bridge 服务或移除 Gitea webhook 配置。
- 不涉及对 Vibe-Kanban 的 schema/代码修改，无主系统回滚成本。

## Open Questions

- `review/init` 所需 email 字段是否使用统一机器人邮箱，还是按仓库配置？
- 是否需要在 Gitea 评论中追加 review 摘要（而不仅是链接）？
- 首期存储选型是 SQLite（部署简单）还是 PostgreSQL（并发更稳）？
- 是否要求在 post-merge 场景中仅处理默认分支合并？
