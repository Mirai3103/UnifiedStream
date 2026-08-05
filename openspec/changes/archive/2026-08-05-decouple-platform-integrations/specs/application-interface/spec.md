# Application interface Specification (Delta)

## MODIFIED Requirements

### Requirement: Existing feature parity
The redesigned interfaces MUST retain access to every existing user action, live value, lifecycle state, permission state, failure reason, and diagnostic currently exposed for discovery, pairing, connection control, camera, microphone, speaker, telemetry, and the synthetic test stream.

Where an action or value is backed by a capability the running platform may not provide, the interface MUST retain it wherever the platform provides it, and MUST omit it entirely where the platform reports the capability as absent. Such an action MUST NOT be presented as available and then fail, and MUST NOT be presented as switched off when it is unavailable.

The interface MUST NOT contain text naming a specific platform, device node, or system component. Platform-specific descriptions, device identifiers, and remediation commands are rendered from values supplied by the backend.

#### Scenario: Existing command remains reachable
- **WHEN** an action was available before the redesign and its existing preconditions are satisfied
- **THEN** the user can invoke the same underlying command or ViewModel callback from an appropriate destination in the redesigned interface

#### Scenario: Existing state remains visible
- **WHEN** the backend or ViewModel reports an active, pending, refused, unavailable, permission-required, reconnecting, disconnected, or failed state
- **THEN** the redesigned interface displays that state and its available recovery action without replacing it with fabricated prototype data

#### Scenario: Navigation during streaming
- **WHEN** the user changes destination while one or more media streams are active
- **THEN** navigation does not stop, restart, renegotiate, mute, or otherwise modify those streams

#### Scenario: An optional platform capability is absent
- **WHEN** the backend reports that the running platform does not provide a capability behind a user action
- **THEN** the interface omits that action rather than showing it disabled, off, or failing

#### Scenario: Platform-specific text is rendered from backend values
- **WHEN** the interface shows a device identifier, a setup instruction, or a description of where media is being delivered
- **THEN** it renders the value supplied by the backend for the running platform
- **AND** any fallback text it substitutes when no value is supplied names no specific platform or system component
