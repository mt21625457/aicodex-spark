## ADDED Requirements

### Requirement: Pull request feedback publication
Bridge MUST publish review lifecycle feedback to the corresponding Gitea pull
request as comments.

#### Scenario: Publish success feedback with review link
- **WHEN** a review job reaches `completed`
- **THEN** bridge posts a PR comment that includes the review URL
- **AND** the comment contains enough context to identify the reviewed
  pull request event

#### Scenario: Publish failure feedback
- **WHEN** a review job reaches `failed` terminal state
- **THEN** bridge posts a PR comment indicating failure
- **AND** the comment includes the job or review identifier for troubleshooting

### Requirement: Feedback publishing idempotency
Bridge MUST avoid duplicate result comments for the same workflow job terminal
state.

#### Scenario: Prevent duplicate success comment
- **WHEN** a completed job is replayed or retried
- **THEN** bridge does not create multiple success comments for the same job

### Requirement: Feedback retry and fallback handling
Bridge MUST retry transient Gitea API failures and preserve unsent feedback for
later reconciliation.

#### Scenario: Retry comment publish on transient error
- **WHEN** Gitea comment API returns transient failure (transport/5xx)
- **THEN** bridge retries with bounded exponential backoff
- **AND** records final failure state if retries are exhausted
