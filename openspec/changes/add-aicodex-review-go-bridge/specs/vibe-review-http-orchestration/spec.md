## ADDED Requirements

### Requirement: Review orchestration via existing Vibe HTTP APIs
Bridge MUST trigger code review using existing Vibe-Kanban public HTTP APIs
without requiring any Vibe-Kanban code changes.

#### Scenario: Successful end-to-end review orchestration
- **WHEN** a valid review workflow job is executed
- **THEN** bridge calls `POST /v1/review/init`
- **AND** uploads repository archive to the returned `upload_url`
- **AND** calls `POST /v1/review/start`
- **AND** stores `review_id` and resulting review URL for later feedback

### Requirement: Repository snapshot preparation
Bridge MUST produce a deterministic repository archive from the target pull
request head commit before calling Vibe APIs.

#### Scenario: Build archive from pull request head
- **WHEN** a review job begins
- **THEN** bridge clones/fetches repository content using configured Gitea
  credentials
- **AND** checks out the pull request head commit
- **AND** creates a `tar.gz` archive for upload

### Requirement: Status polling and terminal state handling
Bridge MUST track review execution state via Vibe status API until terminal
state (`completed` or `failed`) or timeout.

#### Scenario: Review completed
- **WHEN** `GET /v1/review/{id}/status` returns `completed`
- **THEN** bridge marks job as completed
- **AND** triggers success feedback publishing workflow

#### Scenario: Review failed or timed out
- **WHEN** status polling returns `failed` or exceeds configured timeout
- **THEN** bridge marks job as failed
- **AND** triggers failure feedback publishing workflow

### Requirement: Retry policy for transient failures
Bridge MUST retry transient HTTP and network failures with bounded exponential
backoff.

#### Scenario: Retry init/start on transient errors
- **WHEN** `review/init` or `review/start` fails with transient transport or
  `5xx` error
- **THEN** bridge retries according to configured retry policy
- **AND** records each retry attempt in job history
- **AND** moves job to dead-letter state after max attempts
