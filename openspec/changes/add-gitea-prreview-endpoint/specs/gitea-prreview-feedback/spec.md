## ADDED Requirements

### Requirement: Success feedback on pull request
The system MUST publish a success comment to the related Gitea pull request when a review completes.

#### Scenario: Review completed successfully
- **WHEN** the review worker reports completion for a Gitea-triggered review
- **THEN** the system posts a comment on the pull request
- **AND** the comment includes a link to the generated review result

### Requirement: Failure feedback on pull request
The system MUST publish a failure comment to the related Gitea pull request when review processing fails.

#### Scenario: Review failed
- **WHEN** the review workflow reaches a failed terminal state
- **THEN** the system posts a failure comment on the pull request
- **AND** the comment includes a traceable identifier for troubleshooting

### Requirement: Idempotent feedback publishing
The system MUST prevent duplicate feedback comments for the same review terminal state.

#### Scenario: Retry after feedback timeout
- **WHEN** feedback publishing is retried for a terminal state that already has a posted comment
- **THEN** the system does not create an additional duplicate comment
- **AND** the existing feedback record remains authoritative

### Requirement: Feedback retry policy for transient errors
The system MUST retry feedback publishing on transient Gitea API failures with bounded attempts.

#### Scenario: Retry and eventually succeed
- **WHEN** the first feedback publish attempt fails due to a transient upstream error
- **THEN** the system retries publishing according to configured retry policy
- **AND** the system posts exactly one terminal-state feedback comment after recovery

#### Scenario: Exhaust retries and persist failure
- **WHEN** all retry attempts are exhausted
- **THEN** the system records feedback-publish failure state for operator intervention
- **AND** the review terminal state remains queryable
