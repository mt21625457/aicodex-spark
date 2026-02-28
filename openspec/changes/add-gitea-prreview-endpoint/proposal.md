## Why

当前系统仅支持 GitHub webhook 触发代码评审，Gitea 仓库无法通过统一入口接入自动评审流程。  
新增 `/api/v1/gitea/prreview` 可以让 Gitea 在 PR 事件发生时直接触发与 GitHub 相同的“clone -> 打包 -> review worker”链路，减少人工触发与集成成本。

## What Changes

- 新增公开 API 端点 `/api/v1/gitea/prreview`，用于接收 Gitea webhook 并触发代码评审流程。
- 在端点中增加 Gitea webhook 头部与签名校验、事件类型识别、基础幂等处理（按 delivery 去重）。
- 新增 Gitea PR 上下文解析与评审启动编排：解析 PR 元数据、执行仓库 clone、上传评审 payload、调用现有 review worker。
- 新增 Gitea 回写能力：评审成功/失败后向对应 PR 发布评论（链接或失败原因）。
- 为 Gitea 集成增加必要配置与数据存储（例如 token/secret、仓库开关、投递记录）。
- **必须支持配置指定仓库数据目录**：通过 `GITEA_PRREVIEW_REPO_DATA_DIR` 指定 clone 与中间产物的根目录（例如 `/data/aicodex/gitea-prreview`），并在服务启动时校验目录可用性与权限。
- **必须支持启动配置文件**：支持通过配置文件集中声明 webhook、token、存储目录和清理策略，并允许环境变量覆盖关键字段。

## Capabilities

### New Capabilities
- `gitea-prreview-endpoint`: 提供 `/api/v1/gitea/prreview` 的 webhook 接入、鉴权、事件过滤与幂等保障。
- `gitea-prreview-orchestration`: 将 Gitea PR 事件编排为现有 clone 型代码评审流程（初始化、上传、启动、状态跟踪）。
- `gitea-prreview-feedback`: 将评审结果回写到 Gitea PR 评论，确保调用方可见性与可追踪性。
- `gitea-prreview-storage-config`: 提供仓库数据目录配置能力，支持运维指定持久化目录并执行容量/权限治理。

### Modified Capabilities
- None.

## Impact

- Affected backend modules:
  - `crates/remote/src/routes/`（新增或扩展 Gitea PR review 路由）
  - `crates/remote/src/state.rs`（注册 Gitea 服务依赖）
  - `crates/remote/src/config.rs`（新增 Gitea 集成配置）
  - `crates/remote/src/db/`（新增 Gitea 集成与 webhook 幂等相关存储）
  - `crates/remote/src/routes/review.rs`（扩展回写逻辑以支持 Gitea）
- External APIs:
  - 新增 `/api/v1/gitea/prreview` webhook 接口
  - 依赖 Gitea REST API（PR 详情、评论发布）与现有 review worker API
- Required configuration:
  - `GITEA_PRREVIEW_CONFIG_FILE`（可选）：启动时读取配置文件路径（例如 `/etc/aicodex/gitea-prreview.yaml`）。
  - `GITEA_PRREVIEW_REPO_DATA_DIR`（必填）：用于指定仓库 clone 和评审中间文件的根目录（绝对路径）。
  - `GITEA_PRREVIEW_WEBHOOK_SECRET`（必填）：用于验证 `X-Gitea-Signature`。
  - `GITEA_PRREVIEW_TOKEN`（必填）：用于调用 Gitea API（PR 详情、评论发布、仓库访问）。
  - 配置优先级：`环境变量 > 配置文件 > 内置默认值`，并在合并后对必填项执行启动期 fail-fast 校验。
- Operational impact:
  - 需要新增 Gitea webhook secret/token 配置与安全管理
  - 增加 webhook 流量与 clone/打包资源消耗，需要监控并发与失败重试
  - 需要为指定目录做磁盘容量规划、权限控制与清理策略
