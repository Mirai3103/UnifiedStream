# Repository Quality Gates Specification (Delta)

## MODIFIED Requirements

### Requirement: Pull request quality validation
The repository SHALL run automated OpenSpec, Rust, Android, and frontend quality checks for every pull request targeting `main` and SHALL report each area as a distinct check. Where an area is verified on more than one target platform, it SHALL report one distinct check per verified platform, and every one of them SHALL be required.

#### Scenario: Pull request starts validation
- **WHEN** a pull request targeting `main` is opened or updated
- **THEN** the repository runs all four quality areas against that pull request revision

#### Scenario: A quality area fails
- **WHEN** any command in a quality area fails
- **THEN** its distinct check reports failure and identifies the failing command in its logs

#### Scenario: An area verifies several platforms
- **WHEN** a quality area is verified on more than one target platform
- **THEN** each platform reports its own check
- **AND** a failure on any one of them blocks the merge

## ADDED Requirements

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
