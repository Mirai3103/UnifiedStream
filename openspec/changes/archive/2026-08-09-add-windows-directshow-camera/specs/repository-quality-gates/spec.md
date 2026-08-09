## MODIFIED Requirements

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

## ADDED Requirements

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
