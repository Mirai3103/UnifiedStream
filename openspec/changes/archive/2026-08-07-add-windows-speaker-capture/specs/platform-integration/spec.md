## ADDED Requirements

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
