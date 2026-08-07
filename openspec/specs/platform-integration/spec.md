# Platform Integration

## Purpose

Define the portability contract between the desktop application and platform-specific media integrations.
## Requirements
### Requirement: Application layer independent of platform integrations
The desktop application layer SHALL depend only on the media integration traits and MUST NOT name, construct, or call a method on a platform-specific type. Concrete platform types SHALL be referenced only inside a single platform module per integration crate.

#### Scenario: A platform-specific type is used in the application layer
- **WHEN** the application layer names a platform-specific type, imports it, or calls a method that exists on only one platform's implementation
- **THEN** the change is rejected, because the required capability belongs on the trait or behind a platform factory

#### Scenario: A new integration is added
- **WHEN** an integration for another platform is implemented
- **THEN** it is added inside the platform module of the relevant crate
- **AND** no application-layer file changes to accommodate it

### Requirement: Every supported target compiles
The desktop workspace, including the application binary, SHALL compile for every supported target platform whether or not that platform has a media implementation.

#### Scenario: A target without an implementation is built
- **WHEN** the workspace is built for a target that has no platform media implementation
- **THEN** the build succeeds, including the application binary
- **AND** the resulting application starts, discovers, pairs, connects, and reports telemetry

#### Scenario: Conditional compilation is confined
- **WHEN** platform selection is required
- **THEN** it occurs in a platform module of an integration crate
- **AND** not in the application layer

### Requirement: Unsupported integrations degrade visibly
A media integration that has no implementation on the running platform SHALL refuse the stream and SHALL report an unavailable state naming the platform. It MUST NOT panic, MUST NOT fail the build, and MUST NOT silently discard media while reporting the stream as active.

#### Scenario: A stream is requested on a platform without an implementation
- **WHEN** a stream start is received or initiated for a media integration with no implementation on this platform
- **THEN** the stream is refused with reason `internal`
- **AND** the desktop UI shows that the feature is unavailable on this platform

#### Scenario: The session survives an unsupported integration
- **WHEN** every media integration is unavailable on the running platform
- **THEN** discovery, pairing, the session, and telemetry continue to work
- **AND** the failure of one integration does not stop the others from being attempted

### Requirement: Platform-supplied setup guidance
Setup guidance shown when an integration is unavailable SHALL be supplied by the platform implementation as a user-facing message with an optional copyable command. The application layer and the user interface MUST NOT contain platform-specific remediation text.

#### Scenario: A platform reports a fixable failure
- **WHEN** a platform implementation fails to start because of a missing system prerequisite it can name
- **THEN** it supplies the message and, where one exists, the exact command that resolves it
- **AND** the user interface renders both without knowing which platform produced them

#### Scenario: A platform reports an unfixable failure
- **WHEN** a platform implementation fails with no user-actionable command
- **THEN** it supplies the message alone
- **AND** the user interface shows no command

### Requirement: Optional platform capabilities are absent rather than failing
A capability that exists on some platforms and has no meaning on others SHALL be reported as absent on the platforms that lack it, and its control SHALL NOT be offered there.

#### Scenario: A platform lacks an optional capability
- **WHEN** the running platform provides no implementation of an optional capability
- **THEN** the status reported to the user interface marks the capability as absent rather than disabled or off
- **AND** the user interface omits its control entirely

#### Scenario: A platform provides an optional capability
- **WHEN** the running platform provides the capability
- **THEN** its control is offered and behaves as that capability's own requirements specify

### Requirement: Platform support is per integration

Media support SHALL be resolved for each integration independently rather than for a platform as a whole. A platform MAY implement some integrations and not others, and an integration a platform has not implemented SHALL behave exactly as it does on a platform with no implementations at all.

A platform's partial support MUST NOT be visible to the application layer: it obtains every integration from the same factories in the same way, whether that platform implements one integration, all of them, or none.

#### Scenario: A platform implements one integration and not another

- **WHEN** the running platform implements one media integration but has no implementation of another
- **THEN** the implemented one operates normally
- **AND** the unimplemented one refuses the stream and reports an unavailable state naming the platform
- **AND** an optional capability the platform does not provide is reported as absent

#### Scenario: An integration is added to a partially supported platform

- **WHEN** an integration is implemented for a platform that already implements another
- **THEN** the change is confined to that integration's crate
- **AND** no application-layer file changes
- **AND** the platform's remaining unimplemented integrations continue to report as unavailable, unchanged

