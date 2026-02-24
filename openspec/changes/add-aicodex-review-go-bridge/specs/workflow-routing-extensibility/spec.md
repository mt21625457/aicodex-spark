## ADDED Requirements

### Requirement: Configurable workflow routing
Bridge MUST route incoming webhook events to workflows using configuration, not
hard-coded event branching only.

#### Scenario: Route pull request event to review workflow
- **WHEN** routing rules match a pull request event to workflow `review`
- **THEN** bridge enqueues a `review` workflow job

#### Scenario: Ignore unmatched events
- **WHEN** no routing rule matches an incoming event
- **THEN** bridge returns `200 OK`
- **AND** does not enqueue any workflow job

### Requirement: Workflow contract abstraction
Bridge MUST expose an internal workflow interface that allows adding new
workflows without changing webhook ingress core logic.

#### Scenario: Register additional workflow type
- **WHEN** a new workflow implementation is registered
- **THEN** ingress and routing layers can dispatch to it using workflow id
- **AND** existing `review` workflow behavior remains unchanged

### Requirement: Per-repository routing overrides
Bridge MUST support repository-level enable/disable controls for each workflow.

#### Scenario: Disable review workflow for a repository
- **WHEN** repository-level config disables `review` workflow
- **THEN** matching events for that repository do not create review jobs
- **AND** bridge logs skip reason `workflow_disabled_for_repo`
