## Purpose

Define the repository-wide quality checks and protected contribution workflow required to keep `main` stable.

## Requirements

### Requirement: Pull request quality validation
The repository SHALL run automated OpenSpec, Rust, Android, and frontend quality checks for every pull request targeting `main` and SHALL report each area as a distinct check. Where an area is verified on more than one target platform, it SHALL report one distinct check per verified platform, and every one of them SHALL be required.

#### Scenario: Pull request starts validation
- **WHEN** a pull request targeting `main` is opened or updated
- **THEN** the repository runs all four quality checks against that pull request revision

#### Scenario: A quality area fails
- **WHEN** any command in a quality area fails
- **THEN** its distinct check reports failure and identifies the failing command in its logs

#### Scenario: An area verifies several platforms
- **WHEN** a quality area is verified on more than one target platform
- **THEN** each platform reports its own check
- **AND** a failure on any one of them blocks the merge

### Requirement: Non-Linux compilation gate
The Rust quality area SHALL verify that the desktop workspace, including the application binary, compiles and passes lints for a target platform other than Linux, on every pull request targeting `main`.

#### Scenario: A Linux-only dependency reaches the application layer
- **WHEN** a change makes the application layer depend on a Linux-only type, module, or crate
- **THEN** the non-Linux leg of the Rust check fails to compile
- **AND** the merge is blocked before review discipline is relied upon

#### Scenario: The non-Linux leg runs
- **WHEN** the non-Linux leg of the Rust quality check executes
- **THEN** it checks the whole workspace and denies Clippy warnings for workspace production targets
- **AND** it does not run tests that require a Linux audio or video subsystem

### Requirement: Reproducible quality commands
CI SHALL use checked-in wrappers and lockfiles where available, pinned toolchain versions where needed, and the repository's standard validation commands.

#### Scenario: Rust validation runs
- **WHEN** the Rust quality check executes
- **THEN** it checks formatting, denies Clippy warnings for workspace production targets, and runs all workspace tests

#### Scenario: Android validation runs
- **WHEN** the Android quality check executes
- **THEN** it uses the checked-in Gradle wrapper to run debug unit tests and assemble the debug APK

#### Scenario: Frontend validation runs
- **WHEN** the frontend quality check executes
- **THEN** it installs dependencies from the committed Bun lockfile without updating it and completes the production build

#### Scenario: OpenSpec validation runs
- **WHEN** the OpenSpec quality check executes
- **THEN** it runs `openspec validate --all` with a pinned OpenSpec CLI version and fails if any item is invalid

### Requirement: Protected main branch
The GitHub repository MUST reject direct pushes, force pushes, and deletion of `main`, and MUST allow changes into `main` only by merging a pull request whose required quality checks pass.

#### Scenario: Direct push is attempted
- **WHEN** a contributor attempts to push a commit directly to `main`
- **THEN** GitHub rejects the push

#### Scenario: Pull request has a failing required check
- **WHEN** any required quality check on a pull request is failing or incomplete
- **THEN** GitHub prevents that pull request from merging into `main`

#### Scenario: Pull request satisfies protection rules
- **WHEN** all required quality checks pass and the pull request satisfies the configured repository rules
- **THEN** GitHub allows the pull request to be merged into `main`

### Requirement: Documented contribution workflow
The repository SHALL document the required branch-and-pull-request workflow and the local commands contributors can run before opening a pull request.

#### Scenario: Contributor prepares a change
- **WHEN** a contributor consults the repository documentation
- **THEN** they can identify how to create a branch, validate the change locally, open a pull request, and wait for required checks before merge
