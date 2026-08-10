## Purpose

Define the repository-wide quality checks and protected contribution workflow required to keep `main` stable.
## Requirements
### Requirement: Pull request quality validation
The repository SHALL run automated quality checks for every pull request targeting `main`, covering every area of the repository that can be built or validated independently, and SHALL report each area as a distinct check. Where an area is verified on more than one target platform, it SHALL report one distinct check per verified platform, and every one of them SHALL be required.

An area is defined by what builds it, not by what language it is written in. Where a component is built by a toolchain no existing area invokes, it SHALL become its own area with its own required check rather than being assumed to be covered by an existing one — a component no check builds is a component that can be broken by a merge without anything failing.

#### Scenario: Pull request starts validation
- **WHEN** a pull request targeting `main` is opened or updated
- **THEN** the repository runs every quality area against that pull request revision

#### Scenario: A quality area fails
- **WHEN** any command in a quality area fails
- **THEN** its distinct check reports failure and identifies the failing command in its logs

#### Scenario: An area verifies several platforms
- **WHEN** a quality area is verified on more than one target platform
- **THEN** each platform reports its own check
- **AND** a failure on any one of them blocks the merge

#### Scenario: A component is added that no existing area builds
- **WHEN** a component is added whose toolchain no existing quality area invokes
- **THEN** a new area is added that builds it, reported as its own required check
- **AND** the merge is blocked if it fails to build

### Requirement: Non-Linux compilation gate
The Rust quality area SHALL verify that the desktop workspace, including the application binary, compiles and passes lints for a target platform other than Linux, on every pull request targeting `main`. Where that platform has media implementations of its own, the gate SHALL also run the tests that can run there, so platform code is verified on the platform it targets rather than only compiled for it.

#### Scenario: A Linux-only dependency reaches the application layer
- **WHEN** a change makes the application layer depend on a Linux-only type, module, or crate
- **THEN** the non-Linux leg of the Rust check fails to compile
- **AND** the merge is blocked before review discipline is relied upon

#### Scenario: The non-Linux leg runs
- **WHEN** the non-Linux leg of the Rust quality check executes
- **THEN** it checks the whole workspace and denies Clippy warnings for workspace production targets
- **AND** it runs the workspace tests that do not require a subsystem absent from that platform
- **AND** it does not run tests that require a Linux audio or video subsystem

#### Scenario: Platform media code is added without tests running on its platform
- **WHEN** a media implementation is added for the non-Linux platform and its tests are not exercised by that leg
- **THEN** the gate does not report success on the strength of compilation alone

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

### Requirement: Third-party source provenance is recorded before it is incorporated

Where source authored outside this repository is copied into it, adapted, or used as the reference for an implementation, its licence SHALL be established and recorded before that source is read, and SHALL be compatible with the licence this repository publishes under.

The order matters and is the requirement: a licence checked after the fact cannot be acted on, because the knowledge cannot be given back. A permissively licensed reference and a copyleft one lead to the same code and to very different obligations, and the difference is invisible in the result.

The repository SHALL publish its own licence, so that compatibility is a question with an answer rather than a decision made by accident on the first incorporation.

#### Scenario: A third-party reference is chosen

- **WHEN** an implementation is to be based on source authored outside this repository
- **THEN** that source's licence is established and recorded first
- **AND** a source whose licence is incompatible with this repository's is not read

#### Scenario: Third-party source is vendored

- **WHEN** source authored outside this repository is copied into it
- **THEN** its origin and licence are recorded alongside it
- **AND** its licence terms are satisfied by what the repository publishes

#### Scenario: The repository publishes no licence

- **WHEN** third-party source is to be incorporated and the repository declares no licence of its own
- **THEN** the repository's licence is established as part of that change
- **AND** compatibility is assessed against it rather than deferred

### Requirement: Redistributed third-party components carry their required notices

Where the product redistributes a third-party component under a grant conditioned on attribution, identification, or notice, the repository SHALL record what that grant requires and SHALL verify by automated check that the shipped product still satisfies it.

This is distinct from recording provenance for source that is read or vendored, and is not covered by it. A component redistributed as a binary contributes nothing to the source tree: there is no file to record a licence header against, no compiler that sees it, and no test that exercises it. The obligation lives instead in what the product displays to its user, which is the one place a change can silently remove it — deleting a line from an About screen breaks no build, fails no test, and converts a compliant product into an infringing one.

The check SHALL fail when the required notice is absent, and MUST NOT be satisfied by the notice existing only in documentation, in a design record, or in a comment. Where the grant requires that the user be able to act on the notice — to identify the component's author, or to reach the place it may be paid for — the check SHALL verify that the means of acting is present, not merely that the component is named.

Recording the requirement SHALL happen as part of the change that introduces the dependency, for the same reason provenance is established before source is read: an obligation discovered after distribution has already been breached.

#### Scenario: A component is redistributed under a notice obligation

- **WHEN** a change introduces a third-party component that the product redistributes under a grant requiring attribution or notice
- **THEN** what the grant requires is recorded in the repository as part of that change
- **AND** an automated check verifies the shipped product satisfies it

#### Scenario: A required notice is removed

- **WHEN** a change removes or empties a required notice from the product's user-facing surface
- **THEN** the check fails and the merge is blocked
- **AND** the failure identifies which component's obligation is unmet

#### Scenario: A notice names the component but cannot be acted on

- **WHEN** the grant requires the user be able to identify the component's author or reach where it may be paid for, and the product names the component without either
- **THEN** the check fails
- **AND** naming the component alone does not satisfy it

#### Scenario: The obligation is recorded only in a design document

- **WHEN** a redistributed component's notice obligation is described in a design record but no automated check verifies it
- **THEN** the obligation is treated as unverified
- **AND** the change that introduced the component is incomplete

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
