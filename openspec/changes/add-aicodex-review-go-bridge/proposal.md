## Why

当前仓库的自动 PR 审核链路以 GitHub App 为中心，已经无法直接复用到
Gitea。团队需要一个低风险方案，先接入 Gitea webhook 并调用现有
Vibe-Kanban review 能力，同时明确第一阶段不修改原有
Vibe-Kanban 代码（Rust/Frontend）。

## What Changes

- 在 `aicodex-review/` 新增 Go 独立桥接服务（bridge），作为 webhook
  接入与工作流编排层。
- 新增 Gitea webhook 接口能力：签名校验、事件过滤、幂等去重、任务入队。
- 新增“调用 Vibe-Kanban 公开 HTTP API 完成 review”的编排能力：
  `init -> upload -> start -> status`。
- 新增 Gitea 回写能力：在 PR 中发布审核开始/成功/失败评论，并附带
  review 链接。
- 新增可扩展工作流路由机制：在 `review` 之外预留“其他需求”的工作流扩展点
  （例如未来的自动摘要、策略检查、通知分发）。
- 明确约束：第一阶段不改动 `vibe-kanban` 原代码，仅通过 HTTP API
  集成。

## Capabilities

### New Capabilities

- `go-gitea-webhook-ingress`: 接收并验证 Gitea webhook，完成事件过滤与幂等。
- `vibe-review-http-orchestration`: 通过现有 Vibe-Kanban HTTP API
  编排完整 review 流程。
- `gitea-review-feedback`: 将审核执行状态与结果回写到 Gitea PR。
- `workflow-routing-extensibility`: 提供可配置工作流路由，支持后续“其他需求”。

### Modified Capabilities

- None.

## Impact

- 新增代码目录：`aicodex-review/`（Go 工程）。
- 新增运行依赖：Gitea SDK、任务队列/存储（本地 SQLite 或 PostgreSQL）。
- 新增部署单元：Bridge 服务（可独立容器化部署）。
- 需要新增环境变量（Gitea、Vibe API、队列与安全配置）。
- 对现有 `crates/remote`、`frontend`、`remote-frontend` 无代码改动。
