## ADDED Requirements

### Requirement: Configurable repository data directory
The system MUST require an explicit repository data root directory configuration in resolved runtime config (config file and/or environment variables).

#### Scenario: Startup fails when directory is missing after config merge
- **WHEN** the service starts and resolved runtime config does not contain `repo_data_dir`
- **THEN** the service fails fast at startup
- **AND** no webhook processing endpoint is exposed

#### Scenario: Use configured directory for job data
- **WHEN** `repo_data_dir` is configured with a valid absolute path
- **THEN** all clone data and intermediate artifacts are created under that directory tree
- **AND** job metadata stores the concrete directory path used by the workflow

#### Scenario: Startup fails when directory is not absolute or not writable
- **WHEN** resolved `repo_data_dir` is a relative path or points to a non-writable location
- **THEN** the service fails fast at startup
- **AND** startup logs include an actionable configuration error reason

### Requirement: Startup config file and override behavior
The system MUST support loading Gitea PR review storage configuration from a startup config file and applying environment variable override rules.

#### Scenario: Load storage settings from config file
- **WHEN** `GITEA_PRREVIEW_CONFIG_FILE` points to a valid YAML file that includes `gitea_prreview.repo_data_dir`
- **THEN** the service loads the directory and retention settings from that file
- **AND** the storage subsystem uses the loaded values for workflow execution

#### Scenario: Environment variable overrides config file
- **WHEN** both config file and environment variables define `repo_data_dir`
- **THEN** the environment variable value is selected as effective runtime value
- **AND** startup logs indicate config source precedence was applied

#### Scenario: Startup fails for invalid config file
- **WHEN** `GITEA_PRREVIEW_CONFIG_FILE` points to a missing file, unreadable file, or invalid schema
- **THEN** the service fails fast at startup
- **AND** startup logs include actionable parse or schema validation errors

### Requirement: Storage directory security constraints
The system MUST enforce storage path safety and local permission constraints for repository data.

#### Scenario: Reject path traversal
- **WHEN** a computed sub-path would escape the configured root directory
- **THEN** the operation is rejected as invalid
- **AND** the workflow is marked failed with a storage-path error

### Requirement: Capacity and retention control
The system MUST apply configurable capacity and retention policies for repository data directories.

#### Scenario: Reject new job on low disk threshold
- **WHEN** free disk space is below configured minimum threshold
- **THEN** new review jobs are rejected
- **AND** the system emits an operational event indicating low-storage protection

#### Scenario: Cleanup expired job directories
- **WHEN** a job directory exceeds configured retention duration for its terminal state
- **THEN** the cleanup process removes the directory
- **AND** the cleanup action is recorded in audit logs
