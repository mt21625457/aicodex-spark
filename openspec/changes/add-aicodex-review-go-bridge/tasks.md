## 1. Project Bootstrap

- [ ] 1.1 在 `aicodex-review/` 初始化 Go module 与基础目录结构（`cmd/`, `internal/`, `configs/`）。
- [ ] 1.2 引入并封装 Gitea SDK 客户端、Vibe API 客户端与统一配置加载模块。
- [ ] 1.3 选定并接入持久化层（SQLite 或 PostgreSQL）用于幂等与任务状态。

## 2. Webhook Ingress

- [ ] 2.1 实现 `POST /webhook/gitea`，完成 header 校验与签名验证。
- [ ] 2.2 实现 delivery 幂等表与唯一约束（`provider + delivery_id`）。
- [ ] 2.3 实现事件过滤与触发策略（pre-merge / post-merge / both）。
- [ ] 2.4 实现 webhook 快速响应（入队即返回）与审计日志。

## 3. Review Workflow Orchestration

- [ ] 3.1 实现异步 worker：clone/fetch、checkout head commit、打包 tar.gz。
- [ ] 3.2 实现 `review/init -> upload -> review/start` 调用链。
- [ ] 3.3 实现 `review/{id}/status` 轮询与终态状态机（completed/failed/timeout）。
- [ ] 3.4 实现重试、超时和死信处理策略。

## 4. Gitea Feedback Publishing

- [ ] 4.1 实现 review 开始/成功/失败评论模板与发布逻辑。
- [ ] 4.2 实现评论发布幂等控制，避免重复刷屏。
- [ ] 4.3 实现 Gitea API 失败重试与失败补偿记录。

## 5. Workflow Extensibility

- [ ] 5.1 实现 workflow registry 与路由接口抽象。
- [ ] 5.2 以 `review` 作为首个 workflow 落地，保证 ingress 无业务耦合。
- [ ] 5.3 增加 repository 级 workflow 开关配置解析与生效逻辑。

## 6. Observability & Delivery

- [ ] 6.1 增加结构化日志字段（delivery_id, repo, pr, job_id, review_id）。
- [ ] 6.2 增加健康检查、metrics 与关键失败告警。
- [ ] 6.3 编写部署文档与运行手册（配置项、权限、回滚流程）。
- [ ] 6.4 在测试仓库灰度验证后分阶段推广到生产仓库。
