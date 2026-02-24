## ADDED Requirements

### Requirement: Gitea webhook signature and header validation
Bridge MUST validate incoming Gitea webhook requests before any business
processing. Validation MUST include required headers and HMAC signature
verification using the configured webhook secret.

#### Scenario: Reject request without valid signature
- **WHEN** a webhook request is received with missing or invalid signature
- **THEN** the bridge returns `401 Unauthorized`
- **AND** no job is created
- **AND** an audit log entry is emitted with reason `invalid_signature`

#### Scenario: Reject request with missing event metadata
- **WHEN** a webhook request is received without required metadata headers
  (`X-Gitea-Event` or `X-Gitea-Delivery`)
- **THEN** the bridge returns `400 Bad Request`
- **AND** no job is created

### Requirement: Idempotent delivery processing
Bridge MUST enforce idempotency per delivery id to prevent duplicate workflow
execution on webhook retries.

#### Scenario: Ignore duplicate delivery
- **WHEN** a webhook request is received with a delivery id that was already
  processed for provider `gitea`
- **THEN** the bridge returns `200 OK`
- **AND** no new workflow job is enqueued
- **AND** response body indicates the request was deduplicated

### Requirement: Configurable event trigger policy
Bridge MUST support configurable trigger policy for pull request events.

#### Scenario: Trigger in pre-merge mode
- **WHEN** bridge is configured in `pre-merge` mode
- **AND** a pull request event action is one of configured pre-merge actions
- **THEN** bridge enqueues a review workflow job

#### Scenario: Trigger in post-merge mode
- **WHEN** bridge is configured in `post-merge` mode
- **AND** a pull request `closed` event indicates `merged=true`
- **THEN** bridge enqueues a review workflow job
