# Speaker Stream Specification (Delta)

## MODIFIED Requirements

### Requirement: System audio routing

System audio routing is an optional platform capability. On a platform whose audio system requires the desktop to become the system default output in order to capture system audio, the desktop SHALL offer a control that makes the virtual sink the system default output while the speaker stream is active, and SHALL restore the previously selected default output when the stream stops, the session ends, or on application startup after an unclean exit.

On a platform that captures system audio without taking over the default output, the desktop SHALL report the routing capability as absent and SHALL NOT offer the control. Absence MUST be distinguishable from the control being present and switched off.

#### Scenario: Routing sends system audio to the phone

- **WHEN** the user enables the route-system-audio control while the speaker stream is active
- **THEN** the virtual sink becomes the system default output
- **AND** system audio plays on the phone

#### Scenario: The previous output is restored on stop

- **WHEN** the stream stops or the session ends while routing is enabled
- **THEN** the system default output is restored to the device that was selected before routing was enabled

#### Scenario: A stale takeover is repaired on startup

- **WHEN** the desktop application starts and finds the remembered default output was left pointing at its own virtual sink by a previous unclean exit
- **THEN** it restores the persisted previous default output

#### Scenario: A platform that needs no routing

- **WHEN** the desktop runs on a platform that captures system audio without taking over the default output
- **THEN** the speaker status reports routing as absent rather than as off
- **AND** the user interface omits the route-system-audio control
- **AND** the user's selected output device is never changed
