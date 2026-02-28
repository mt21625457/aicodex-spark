## ADDED Requirements

### Requirement: Gitea PR review webhook endpoint
The system MUST provide a public webhook endpoint at `POST /api/v1/gitea/prreview` for Gitea-triggered code review workflows.

#### Scenario: Accept valid pull request webhook
- **WHEN** the endpoint receives a valid Gitea `pull_request` webhook with supported action
- **THEN** the system returns `202 Accepted`
- **AND** a review job is enqueued for asynchronous processing

### Requirement: Webhook authenticity and metadata validation
The system MUST validate Gitea webhook authenticity and required metadata before creating any review job.

#### Scenario: Reject invalid signature
- **WHEN** the request signature does not match the configured webhook secret
- **THEN** the system returns `401 Unauthorized`
- **AND** no review job is created

#### Scenario: Reject missing required headers
- **WHEN** the request is missing required headers (`X-Gitea-Delivery`, `X-Gitea-Event`, or `X-Gitea-Signature`)
- **THEN** the system returns `400 Bad Request`
- **AND** no review job is created

#### Scenario: Ignore unsupported event type
- **WHEN** the request event type is not in the configured trigger policy
- **THEN** the system returns `200 OK`
- **AND** no review job is created

### Requirement: Idempotent delivery handling
The system MUST deduplicate webhook deliveries using the provider and delivery id.

#### Scenario: Ignore duplicate delivery
- **WHEN** the endpoint receives a delivery id that has already been processed for provider `gitea`
- **THEN** the system returns `200 OK` without creating a new job
- **AND** the response indicates the request was deduplicated
