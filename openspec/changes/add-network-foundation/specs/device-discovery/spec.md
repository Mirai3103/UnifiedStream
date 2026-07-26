## ADDED Requirements

### Requirement: Desktop advertises a discoverable service

The desktop application SHALL advertise itself on the local network using mDNS/DNS-SD with service type `_unifiedstream._udp.local.` while discovery is enabled.

#### Scenario: Service is advertised on startup

- **WHEN** the desktop application starts with discovery enabled
- **THEN** it registers an mDNS service of type `_unifiedstream._udp.local.` on all non-loopback network interfaces
- **AND** the service instance name defaults to the machine hostname

#### Scenario: Advertisement is withdrawn on shutdown

- **WHEN** the desktop application exits or the user disables discovery
- **THEN** it sends an mDNS goodbye packet unregistering the service
- **AND** the service no longer appears in browsers on the network within 5 seconds

#### Scenario: Service record carries capability metadata

- **WHEN** the desktop advertises its service
- **THEN** the TXT record contains key `ver` with the protocol version, `name` with a human-readable device name, `id` with the stable device UUID, and `caps` with a comma-separated list of supported stream capabilities
- **AND** the SRV record advertises the TCP control port

### Requirement: Phone discovers desktops on the network

The Android application SHALL browse for `_unifiedstream._udp.local.` services and present discovered desktops to the user without requiring manual IP entry.

#### Scenario: Discovered device appears in the list

- **WHEN** the user opens the device list screen and a desktop is advertising on the same network
- **THEN** the desktop appears in the list within 3 seconds showing its human-readable name and supported capabilities

#### Scenario: Device is removed when it disappears

- **WHEN** a previously discovered desktop stops advertising or sends a goodbye packet
- **THEN** it is removed from the device list within 5 seconds

#### Scenario: Multicast lock is held while browsing

- **WHEN** the Android application begins browsing for services
- **THEN** it acquires a Wi-Fi `MulticastLock` before starting the browse
- **AND** releases the lock when browsing stops

#### Scenario: Duplicate advertisements are collapsed

- **WHEN** the same desktop is discovered on more than one network interface and resolves to multiple addresses
- **THEN** the device list shows exactly one entry for that device, keyed by its TXT `id`

### Requirement: Manual address entry fallback

The Android application SHALL allow the user to connect by entering a desktop's IP address and port manually when discovery does not find the target device.

#### Scenario: Fallback is offered after discovery timeout

- **WHEN** browsing has been active for 10 seconds and no devices have been found
- **THEN** the UI presents a manual address entry option

#### Scenario: Manual connection succeeds

- **WHEN** the user enters a valid IP address and control port of a running desktop and confirms
- **THEN** the application attempts a control-channel connection to that address
- **AND** on success the device is treated identically to a discovered device

#### Scenario: Invalid address is rejected

- **WHEN** the user enters a malformed IP address or a port outside 1-65535
- **THEN** the application shows a validation error and does not attempt a connection

### Requirement: Stable device identity

Each application instance SHALL generate a stable device identifier on first run and persist it across restarts.

#### Scenario: Identifier is generated once

- **WHEN** the application starts for the first time
- **THEN** it generates a random UUID, persists it to local storage, and uses it as its device identifier

#### Scenario: Identifier survives restart

- **WHEN** the application restarts after having generated an identifier
- **THEN** it loads the previously persisted identifier rather than generating a new one

### Requirement: Discovery reports network unavailability

The applications SHALL surface a distinct state when discovery cannot run because the device is not connected to a usable network.

#### Scenario: No Wi-Fi connection

- **WHEN** the user opens the device list while the phone has no Wi-Fi connection
- **THEN** the UI shows a "not connected to a network" state rather than an empty device list
