## Context

当前远端评审链路以 GitHub webhook 为主，仓库 clone 使用临时目录策略，未暴露可配置的“仓库数据存储目录”。  
本次新增 `/api/v1/gitea/prreview` 后，Gitea 事件会持续触发 clone、打包、上传和回写流程，如果没有明确的目录配置与生命周期策略，会带来以下问题：

- 无法进行磁盘容量规划与告警。
- 无法稳定复现失败现场（目录不可预测或过早清理）。
- 并发下可能出现目录冲突、脏数据残留、清理不彻底。
- 运维侧无法按环境（开发/测试/生产）精细化配置存储行为。

因此，本设计将“仓库数据存储目录”作为新增接口的基础设施能力，而非实现细节。

## Goals / Non-Goals

**Goals:**

- 新增 `/api/v1/gitea/prreview`，可接收 Gitea webhook 并触发 clone 型代码评审。
- 提供可配置、可观测、可清理的仓库数据存储目录体系。
- 明确目录结构、命名规则、权限模型、容量限制、生命周期策略。
- 在失败场景下保证目录可追踪（支持排障）且最终可回收（防止磁盘泄漏）。

**Non-Goals:**

- 不在本次设计中重构 GitHub webhook 既有实现。
- 不引入跨机共享文件系统（NFS/Ceph）作为首期依赖。
- 不在首期实现仓库长期缓存加速（可预留扩展位）。

## Decisions

### 1) API 入口与触发模型

**Decision**

- 新增公开 webhook 接口：`POST /api/v1/gitea/prreview`。
- 首期处理 `pull_request`（opened/reopened/synchronized）与 `issue_comment`（`!reviewfast`）事件。
- webhook 线程只做：签名校验、事件解析、幂等判重、任务入队；重操作异步执行。

**Rationale**

- 路径语义明确，便于 Gitea 侧直接配置。
- 与现有异步评审模式一致，避免 webhook 超时和重放风暴。

**Alternatives**

- 复用现有 GitHub 路由：provider 语义混杂，不利于后续维护。
- 同步执行 clone+review：实现简单但稳定性差。

### 2) 仓库数据存储目录采用“单根目录 + 分层子目录”模型

**Decision**

新增配置根目录：`GITEA_PRREVIEW_REPO_DATA_DIR`（必须为绝对路径）。  
在根目录下按固定结构管理所有中间数据：

```text
${GITEA_PRREVIEW_REPO_DATA_DIR}/
  deliveries/                 # 原始 webhook 请求体与头信息（短期保留）
    <yyyy>/<mm>/<dd>/<delivery_id>.json
  jobs/
    <job_id>/
      repo/                   # clone 后仓库工作目录
      payload/                # 打包产物、元数据清单
      state.json              # 本地任务状态快照
  locks/                      # 并发锁文件（repo 维度）
  tmp/                        # 临时文件
  quarantine/                 # 清理失败或可疑目录隔离区
```

**Rationale**

- 目录结构可预期，便于追踪和审计。
- `job_id` 隔离避免并发任务互相污染。
- `deliveries` 保留可支持幂等、排障与安全追踪。

**Alternatives**

- 全部使用 `tempfile`：简单但缺少容量控制与可观测性。
- 按仓库固定目录复用：并发冲突与脏状态风险高。

### 3) 仓库数据目录运行时配置项（支持配置文件 + 环境变量）

**Decision**

新增配置加载机制：

- 支持可选启动配置文件 `GITEA_PRREVIEW_CONFIG_FILE`，文件格式为 YAML。
- 若设置了配置文件路径，服务在启动阶段读取并校验配置结构。
- 配置合并优先级固定为：`环境变量 > 配置文件 > 内置默认值`。
- 合并后必填字段 `repo_data_dir`、`webhook_secret`、`token` 缺失时，服务 fail-fast 拒绝启动。

配置文件示例（`/etc/aicodex/gitea-prreview.yaml`）：

```yaml
gitea_prreview:
  repo_data_dir: /data/aicodex/gitea-prreview
  webhook_secret: ${GITEA_WEBHOOK_SECRET}
  token: ${GITEA_TOKEN}
  retention_hours: 24
  delivery_retention_hours: 72
  max_total_gb: 20
  min_free_gb: 5
  max_repo_size_mb: 2048
  cleanup_interval_seconds: 300
  keep_failed_hours: 12
```

新增以下运行时配置（可来自配置文件，且可被同名环境变量覆盖）：

- `GITEA_PRREVIEW_REPO_DATA_DIR`  
  仓库数据根目录；必填；绝对路径；启动时自动创建并校验权限。
- `GITEA_PRREVIEW_RETENTION_HOURS`（默认 `24`）  
  `jobs/*` 与 `deliveries/*` 的默认保留时长。
- `GITEA_PRREVIEW_DELIVERY_RETENTION_HOURS`（默认 `72`）  
  webhook 原始投递保留时长；覆盖默认保留策略。
- `GITEA_PRREVIEW_MAX_TOTAL_GB`（默认 `20`）  
  目录总空间软上限；超过后触发主动清理与拒绝新任务策略。
- `GITEA_PRREVIEW_MIN_FREE_GB`（默认 `5`）  
  所在磁盘最小可用空间阈值；低于阈值直接拒绝新任务并报警。
- `GITEA_PRREVIEW_MAX_REPO_SIZE_MB`（默认 `2048`）  
  单任务 clone 后仓库大小上限，超限失败并回写说明。
- `GITEA_PRREVIEW_CLEANUP_INTERVAL_SECONDS`（默认 `300`）  
  后台清理任务执行周期。
- `GITEA_PRREVIEW_KEEP_FAILED_HOURS`（默认 `12`）  
  失败任务目录最短保留时长，保障问题排查。

**Rationale**

- 配置覆盖容量、生命周期、并发稳定性三类核心控制面。
- 可在不改代码前提下适配不同环境的磁盘约束。

**Alternatives**

- 仅一个 `data_dir` 参数：控制面不足，生产不可运维。

### 4) 目录安全与权限策略

**Decision**

- 根目录及子目录默认权限 `0700`，文件默认 `0600`。
- 路径拼接必须做 canonicalize，禁止 `..` 与符号链接逃逸。
- 仅允许在 `GITEA_PRREVIEW_REPO_DATA_DIR` 子树内读写和删除。
- 清理时采用“先移动到 `quarantine/` 再异步删除”策略，降低误删风险。

**Rationale**

- webhook 载荷属于外部输入，目录处理必须默认不信任。
- 最小权限和路径约束可显著降低目录穿越与数据泄露风险。

**Alternatives**

- 直接 `rm -rf`：实现简单但风险不可控。

### 5) 任务状态与目录生命周期绑定

**Decision**

任务状态机与目录生命周期绑定：

- `accepted`：仅写入 `deliveries/*`
- `running`：创建 `jobs/<job_id>/repo` 与 `jobs/<job_id>/payload`
- `completed`：保留到 `RETENTION_HOURS`
- `failed`：保留到 `KEEP_FAILED_HOURS`
- `cleaned`：目录删除并写入审计日志

每个任务记录 `repo_data_path`、`payload_path`、`cleanup_after`，用于定时清理与审计。

**Rationale**

- 有状态清理可避免“任务仍在执行但目录被回收”的事故。
- 运维可通过 job 记录快速定位目录占用来源。

**Alternatives**

- 纯文件系统扫描清理：容易误删正在运行任务。

### 6) 失败与回滚行为

**Decision**

- 启动阶段若配置文件不可解析、字段类型非法、或 `GITEA_PRREVIEW_REPO_DATA_DIR` 不可写，服务拒绝启动（fail-fast）。
- 运行阶段若空间阈值触发：
  - 拒绝新任务（429/503）并记录可观测事件；
  - 仍允许清理任务运行并优先回收最旧成功任务目录。
- 任务失败时必须保证：
  - 状态持久化为 failed；
  - 目录保留窗口生效；
  - PR 回写失败原因（可操作信息）。

**Rationale**

- 磁盘不足属于系统级故障，隐式降级会扩大故障面。

## Risks / Trade-offs

- [Risk] 目录保留策略过长导致磁盘压力  
  → Mitigation: 默认短保留 + 总量阈值 + 最小可用空间阈值双重控制。
- [Risk] 目录策略过严影响排障（数据过早删除）  
  → Mitigation: 失败任务单独保留窗口 + 可配置延长。
- [Risk] 并发清理和运行任务发生竞争  
  → Mitigation: 状态机锁定 + 仅清理 terminal 状态任务。
- [Risk] 新增配置项增多，部署复杂度上升  
  → Mitigation: 提供安全默认值，仅 `REPO_DATA_DIR` 必填。

## Migration Plan

1. 新增配置定义、配置文件解析与合并逻辑、启动校验与文档（含默认值和示例）。
2. 在 Gitea PR review 执行链路中落地目录创建、路径记录、清理计划。
3. 增加后台清理任务与容量阈值检查。
4. 增加观测指标：
   - 当前目录占用（GB）
   - 任务目录数量（running/completed/failed）
   - 清理成功/失败次数
   - 空间阈值触发次数
5. 在测试环境进行容量压测与故障演练（磁盘不足、清理失败、并发重放）。
6. 灰度上线：单仓库启用 -> 多仓库放量。

Rollback:

- 关闭 Gitea webhook 入口开关并移除 Gitea webhook 配置。
- 保留数据目录，不执行破坏性清理；待人工确认后回收。

## Open Questions

- `/api/v1/gitea/prreview` 是否需要同时提供 `/v1/gitea/prreview` 兼容别名？
- 仓库目录是否需要按组织/仓库维度追加硬配额（而非仅全局阈值）？
- 首期是否需要支持“手动保留某次失败任务目录不自动清理”开关？
