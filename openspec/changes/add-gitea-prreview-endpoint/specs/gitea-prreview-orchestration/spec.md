## ADDED Requirements

### Requirement: Clone-based review orchestration for Gitea PR
The system MUST orchestrate Gitea PR reviews using the existing clone-based flow (clone, archive, upload, start review worker).

#### Scenario: Start review successfully from pull request event
- **WHEN** a valid and deduplicated `pull_request` event is accepted
- **THEN** the system clones the target repository at the PR head revision
- **AND** the system uploads the generated payload archive
- **AND** the system starts the review worker with the created review id

### Requirement: Review trigger from comment command
The system MUST support manual re-trigger via PR comment command.

#### Scenario: Trigger review from `!reviewfast` comment
- **WHEN** a valid `issue_comment` webhook on a pull request contains `!reviewfast`
- **THEN** the system creates a new review workflow job
- **AND** the system enforces deduplication against currently pending jobs for the same PR

### Requirement: Terminal failure handling
The system MUST persist terminal failure status when orchestration fails at any stage.

#### Scenario: Clone stage fails
- **WHEN** repository clone fails due to authentication, network, or repository access error
- **THEN** the system marks the review workflow as failed
- **AND** the failure reason is recorded for feedback publishing

