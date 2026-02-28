## 1. Configuration and startup validation

- [x] 1.1 Extend remote config model with `gitea_prreview` settings (webhook secret, token, repo data dir, retention, capacity, cleanup interval).
- [x] 1.2 Implement startup config file loading from `GITEA_PRREVIEW_CONFIG_FILE` (YAML) and merge with precedence `env > file > defaults`.
- [x] 1.3 Add fail-fast startup validation for required merged settings (`repo_data_dir`, `webhook_secret`, `token`) and absolute/writable storage directory constraints.

## 2. Webhook endpoint and authenticity checks

- [x] 2.1 Add `POST /api/v1/gitea/prreview` route and request parsing for required Gitea headers/body.
- [x] 2.2 Implement signature verification and event filtering for supported actions (`pull_request` opened/reopened/synchronized, `issue_comment` with `!reviewfast`).
- [x] 2.3 Implement delivery id idempotency (provider `gitea` + `X-Gitea-Delivery`) with duplicate short-circuit response behavior.

## 3. Review orchestration with storage directory model

- [x] 3.1 Implement per-job directory provisioning under configured repo data root (`deliveries/`, `jobs/<job_id>/repo`, `jobs/<job_id>/payload`, `locks/`, `tmp/`, `quarantine/`).
- [x] 3.2 Implement clone -> archive/upload -> review worker start flow using parsed Gitea PR metadata.
- [x] 3.3 Persist job state transitions and storage paths (`repo_data_path`, `payload_path`, `cleanup_after`) for audit and cleanup.

## 4. Feedback publishing and retry policy

- [x] 4.1 Implement success feedback comment publishing to target Gitea pull request with review result link.
- [x] 4.2 Implement failure feedback comment publishing with traceable identifier and actionable failure reason.
- [x] 4.3 Add idempotent feedback records and bounded retry policy for transient Gitea API failures.

## 5. Capacity control, cleanup, and path safety

- [x] 5.1 Add disk threshold guards (`max_total_gb`, `min_free_gb`) to reject new jobs and emit operational events.
- [x] 5.2 Implement periodic cleanup worker for expired directories with state-aware retention windows (`retention_hours`, `delivery_retention_hours`, `keep_failed_hours`).
- [x] 5.3 Enforce canonical path safety rules (no traversal/symlink escape) and quarantine-first deletion workflow.

## 6. Tests and operational documentation

- [x] 6.1 Add unit tests for config file parsing, precedence merge, and fail-fast validation errors.
- [x] 6.2 Add endpoint tests for signature failure, missing headers, unsupported events, valid acceptance, and duplicate delivery handling.
- [x] 6.3 Add orchestration/feedback tests for clone failure terminal state, success/failure comment behavior, retry success, and retry exhaustion.
- [x] 6.4 Update operator docs with startup config file example, env override rules, and recommended storage directory sizing/permissions.
