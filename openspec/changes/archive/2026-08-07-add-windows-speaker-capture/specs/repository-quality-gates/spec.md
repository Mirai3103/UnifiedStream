## MODIFIED Requirements

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
