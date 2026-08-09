## Purpose

The behavioural contract of a virtual camera component that consuming applications load into their own processes, rather than a camera device the operating system provides. It covers what such a component must do as a guest in another application's address space, how it declares the frame format version it speaks, how it behaves while the producer is idle, stopped, or gone, and what installing and removing it must leave behind. `camera-stream` states the producing desktop's obligations; this states the consumer's, because the consumer is a separately built and separately installed artifact that can fail entirely on its own.

## Requirements

### Requirement: The component presents the camera to applications of every supported architecture

Where the platform allows applications of more than one processor architecture, and an application can only load a component of its own architecture, the component SHALL be built and installed for every such architecture. A component installed for some but not all of them SHALL NOT be reported as installed, because applications of the remaining architectures cannot see the camera at all.

Each installed architecture SHALL present the camera under the same user-visible name, so the camera has one identity across the product regardless of which application is looking for it.

#### Scenario: Applications of both architectures see the camera

- **WHEN** the component is installed for every supported application architecture and a stream is active
- **THEN** an application of either architecture that enumerates cameras finds one named "UnifiedStream Camera"
- **AND** selecting it delivers the phone's video

#### Scenario: Only one architecture is installed

- **WHEN** the component is installed for one application architecture and not the other
- **THEN** the desktop reports the installation as incomplete rather than as usable
- **AND** the guidance names the architecture that is missing and the command that installs it

### Requirement: The component answers the platform's standard capability negotiation

Applications discover what a camera offers through whatever interface the platform defines for that purpose, and select a format through the same one. They do not inspect the component directly. The component SHALL implement that interface: it SHALL report the complete set of geometries and frame rates it offers, SHALL accept any selection drawn from that set, and SHALL then deliver frames matching the selection.

Answering only the platform's *enumeration* interface is not sufficient and is a distinct failure from not being installed. A component that enumerates but cannot negotiate appears in every application's device list and opens in none of them; the user sees a camera that is present and permanently broken, and the application reports a device error indistinguishable from a camera another program is holding.

#### Scenario: An application reads the offered formats

- **WHEN** an application queries the component for the formats it supports
- **THEN** the component reports every geometry it offers, each with the frame rate it will deliver
- **AND** each reported entry describes the same format the component would deliver if that entry were selected

#### Scenario: An application selects an offered format

- **WHEN** an application selects one of the reported formats before starting the stream
- **THEN** the component accepts the selection
- **AND** reporting the current format afterwards returns the selected one
- **AND** every frame delivered matches the selected geometry

### Requirement: The component never stalls the application hosting it

The component runs inside an application it does not control and MUST NOT make that application's fate depend on the desktop's. It SHALL continue to satisfy its host's requests for video whatever the producer is doing, and SHALL NOT wait indefinitely on the producer, on a shared resource the producer holds, or on the arrival of a frame.

Every producer state other than "delivering new frames" SHALL result in the component supplying a frame of its own choosing — the last frame it received, or a placeholder — and never in an absence of frames.

#### Scenario: The producer is running but idle

- **WHEN** a stream is active and no new frame has been published since the last one the component delivered
- **THEN** the component keeps delivering video to its host at the expected rate
- **AND** the picture holds the last frame received

#### Scenario: The producer stops cleanly

- **WHEN** the desktop stops the camera stream while an application is consuming the camera
- **THEN** the component observes the stop without waiting for a timeout to expire
- **AND** it keeps delivering video to its host, showing a placeholder

#### Scenario: The producer disappears without stopping

- **WHEN** the desktop terminates abruptly while an application is consuming the camera
- **THEN** the component concludes within a bounded time that the producer is gone
- **AND** it keeps delivering video to its host, showing a placeholder
- **AND** the hosting application does not hang, and is not left holding a resource it cannot release

#### Scenario: No producer has ever run

- **WHEN** an application selects the camera while no desktop stream has ever been started
- **THEN** the component starts successfully and delivers a placeholder
- **AND** it begins delivering frames when a stream later starts, without the application reselecting the camera

### Requirement: The component declares the frame format version it speaks

Installation SHALL record the frame format version the component was built against, where the desktop can read it without loading the component and before a stream is started. A component that records no version SHALL be treated as speaking a version the desktop does not, rather than as speaking the current one.

This exists because the desktop is the producer and never reads anything the component writes: a version disagreement is otherwise undiscoverable until frames are already being misread.

#### Scenario: The declared version matches

- **WHEN** the desktop reads the installed component's declared frame format version and it matches its own
- **THEN** the camera stream is allowed to start

#### Scenario: The declared version differs

- **WHEN** the installed component declares a frame format version the desktop does not speak
- **THEN** the desktop refuses the stream before creating any shared resource
- **AND** the guidance names the incompatible component and what resolves it

#### Scenario: No version was recorded

- **WHEN** the component is installed but no frame format version is recorded for it
- **THEN** it is treated as incompatible rather than as current

### Requirement: The component refuses a frame format it does not recognise

On attaching to the producer's shared frame region, the component SHALL verify that the region belongs to this product, that its format version is one the component understands, and that its self-described geometry is internally consistent, before reading a single frame from it. Any of these failing SHALL cause the component to decline to deliver video rather than interpret bytes whose meaning it is guessing at.

The three failures are distinguishable and SHALL NOT be collapsed: a region belonging to another product, a region of a version the component does not speak, and a region that is internally inconsistent call for different messages.

#### Scenario: The region belongs to another product

- **WHEN** the component attaches to a shared region that does not carry this product's identifier
- **THEN** it declines to deliver video from it
- **AND** it reports that the region is not this product's, rather than that the version is wrong

#### Scenario: The region is a version the component does not speak

- **WHEN** the component attaches to a region carrying this product's identifier and a format version it was not built against
- **THEN** it declines to deliver video from it
- **AND** no frame is presented to the application that selected the camera

#### Scenario: The region contradicts itself

- **WHEN** the region's declared layout does not match what the component's format version defines
- **THEN** the component declines to deliver video from it rather than reading at the offsets it assumed

### Requirement: The component discards a frame it could not read intact

The producer never waits for the component, so the component MAY be overtaken part-way through reading a frame. It SHALL detect that this happened and SHALL discard the affected data unread rather than presenting a picture assembled from two different frames.

A discarded frame SHALL cost exactly that frame. It SHALL NOT end the stream, SHALL NOT cause the component to reattach, and SHALL NOT prevent the next frame from being delivered.

#### Scenario: The producer overtakes a read in progress

- **WHEN** the producer writes over the frame the component is copying
- **THEN** the component discards what it copied
- **AND** the picture holds the previous frame rather than showing a spliced one

#### Scenario: A discarded frame does not break the stream

- **WHEN** the component has discarded a frame it could not read intact
- **THEN** the next frame the producer publishes is delivered normally

#### Scenario: The producer is mid-write when the component looks

- **WHEN** the component finds the producer currently writing the frame it would read
- **THEN** it delivers the previous frame for that interval rather than waiting for the write to finish

### Requirement: The component follows geometry changes without being reselected

The frame geometry MAY change while an application is consuming the camera, and the component SHALL observe the change and present frames at the new geometry. It MUST NOT interpret a geometry change as a run of malformed frames, and MUST NOT require the consuming application to reselect the camera.

Where the component's host cannot accept a geometry change on a running connection, the component SHALL renegotiate with its host rather than deliver frames whose size does not match what the host was told to expect.

#### Scenario: The resolution changes mid-stream

- **WHEN** the user selects a different camera resolution while an application is consuming the camera
- **THEN** the component presents subsequent frames at the new resolution
- **AND** it does not report the frames around the change as corrupt

#### Scenario: A stream restarts under a consuming application

- **WHEN** the desktop stops and restarts the camera stream while an application is consuming the camera
- **THEN** the component observes that the frames it is reading belong to a new stream
- **AND** it discards any geometry it had cached from the previous one

### Requirement: The component decodes no encoded media

The component SHALL receive frames in an uncompressed format and MUST NOT contain a decoder for network-originated encoded media. Decoding SHALL happen on the producing side of the process boundary.

The component is loaded into arbitrary applications that have made no decision to trust this product's network input. A decoder is the largest crash and attack surface in this feature, and it stays where a fault costs one process this product owns.

#### Scenario: Frames cross the boundary uncompressed

- **WHEN** the desktop receives an encoded frame from the phone
- **THEN** the desktop decodes it
- **AND** the component receives only the decoded result

#### Scenario: A frame that cannot be decoded

- **WHEN** an arriving frame fails to decode
- **THEN** it is counted and dropped by the desktop
- **AND** nothing about that frame reaches the component

### Requirement: Installation and removal are complete and reversible

Installing the component SHALL register everything needed for applications to enumerate and load it, for every supported architecture, and SHALL record its declared frame format version. Removing it SHALL remove everything installation created.

A registration left behind after removal SHALL be treated as a defect and not as a harmless remnant: it leaves a camera in every application's device list that cannot be loaded.

#### Scenario: Installation makes the camera enumerable

- **WHEN** the component is installed for an architecture
- **THEN** applications of that architecture enumerate the camera
- **AND** the desktop's availability check finds it and its declared version

#### Scenario: Removal leaves nothing behind

- **WHEN** the component is removed
- **THEN** no application enumerates the camera
- **AND** the desktop's availability check reports it as absent rather than as present and broken

#### Scenario: The component is registered but its files are gone

- **WHEN** the component's registration exists but the files it names are not present
- **THEN** the desktop reports the component as absent rather than as installed
- **AND** the guidance names the command that installs it
