# Linux MVP distribution

## Purpose

Define the documentation, installable artifacts, guarded prerelease process, and troubleshooting required to distribute and operate the experimental UnifiedStream Linux MVP with its Android companion.

## Requirements

### Requirement: GitHub-ready user documentation
Repository MUST provide a root README in GitHub Flavored Markdown that identifies Linux as the current desktop MVP, states the Android companion requirement, summarizes supported camera, microphone, and speaker flows, and links to detailed Linux setup and troubleshooting documentation using valid relative links.

#### Scenario: New user opens the repository
- **WHEN** a user opens the repository root on GitHub
- **THEN** the rendered README presents prerequisites, a concise quick start, supported MVP features, known scope limits, and links to the detailed setup and troubleshooting pages

#### Scenario: User follows documentation navigation
- **WHEN** a user selects a setup or troubleshooting link from the root README
- **THEN** GitHub opens the corresponding versioned Markdown document in the repository without a broken relative link

### Requirement: Practical Linux system setup
The Linux setup documentation SHALL cover `v4l2loopback`, PipeWire, firewall, mDNS, and device permissions, and each topic MUST include its purpose, installation or configuration steps, a verification command or observable result, and a safe reversal step where the instructions change persistent system state.

#### Scenario: User prepares virtual camera support
- **WHEN** a user follows the `v4l2loopback` section on a documented Linux distribution
- **THEN** the user can install and load the module, identify the created `/dev/video*` device, verify it, and undo persistent module configuration

#### Scenario: User prepares PipeWire audio support
- **WHEN** a user follows the PipeWire section on a documented Linux distribution
- **THEN** the user can verify the PipeWire user services and confirm whether the UnifiedStream virtual audio nodes are available during a session

#### Scenario: User configures LAN discovery and connectivity
- **WHEN** local firewall or mDNS configuration is required
- **THEN** the documentation identifies the verified protocols and ports, limits example rules to a trusted LAN, provides discovery verification, and explains how to remove any added firewall rule

#### Scenario: User grants device access
- **WHEN** UnifiedStream lacks access to a required video or audio device
- **THEN** the documentation provides least-privilege group, udev, or session-level guidance and does not instruct the user to apply world-writable permissions

### Requirement: End-to-end usage guide
Documentation SHALL describe the ordered steps to install both applications, place both devices on the same trusted LAN, start discovery or use the existing manual-address fallback, approve required Android permissions, connect, activate each stream independently, and verify the resulting Linux camera and audio devices in another application.

#### Scenario: User completes the quick start
- **WHEN** a user has a supported Linux machine and Android device on the same trusted LAN and follows the documented quick start
- **THEN** the user can connect the devices and verify at least one media stream without consulting source code or developer-only commands

#### Scenario: Automatic discovery is unavailable
- **WHEN** mDNS discovery does not list the peer but direct LAN connectivity is available
- **THEN** the usage guide directs the user to the existing manual-address connection path and links to discovery troubleshooting

### Requirement: Versioned Linux MVP artifacts
Each experimental release MUST provide a Tauri AppImage, a Debian package, an installable Android APK, and a `SHA256SUMS` file. Artifact names and release metadata SHALL identify the UnifiedStream version, target platform or architecture, package type, and debug status when an APK is not production-signed.

#### Scenario: Maintainer builds release artifacts
- **WHEN** the prerelease build completes for a valid release revision
- **THEN** it produces the required desktop packages, Android APK, and checksum manifest without relying on uncommitted local files

#### Scenario: User verifies a downloaded artifact
- **WHEN** a user runs the documented SHA-256 verification command in a directory containing an unchanged release artifact and `SHA256SUMS`
- **THEN** checksum verification succeeds for that artifact

#### Scenario: APK uses debug signing
- **WHEN** the experimental APK is signed with a debug key
- **THEN** its filename and release notes explicitly label it as a debug, non-production artifact

### Requirement: Quality-gated GitHub prerelease
The release process MUST create a GitHub prerelease only from a commit on `main` for which the required `openspec`, `rust`, `android`, and `frontend` checks have passed. It MUST use a SemVer prerelease tag and attach only artifacts built from that exact revision.

#### Scenario: Required check has not passed
- **WHEN** any required check for the selected revision is failing, cancelled, or incomplete
- **THEN** the release process stops before publishing or uploading a GitHub prerelease

#### Scenario: Eligible revision is released
- **WHEN** all required checks pass for the selected `main` revision and the release workflow succeeds
- **THEN** GitHub exposes a prerelease with a SemVer prerelease tag and the complete artifact set built from that revision

#### Scenario: Release notes are rendered
- **WHEN** a user opens the GitHub prerelease
- **THEN** release notes identify system requirements, installation steps, MVP limitations, known issues, checksum verification, and links to the full documentation

### Requirement: Symptom-oriented troubleshooting
Repository SHALL provide concise troubleshooting entries for failed mDNS discovery, blocked LAN connectivity, missing or inaccessible `v4l2loopback` devices, missing PipeWire nodes, denied Android permissions, and AppImage startup failures. Each entry MUST include the visible symptom, a diagnostic step, likely cause, corrective action, and confirmation step.

#### Scenario: User diagnoses a common failure
- **WHEN** a user selects a troubleshooting entry matching an observed symptom
- **THEN** the entry provides commands or UI checks that distinguish the common causes and ends with a way to confirm recovery

#### Scenario: Suggested fix requires elevated access
- **WHEN** a troubleshooting action requires root privileges or changes firewall, group, module, or service configuration
- **THEN** the entry marks the elevated command explicitly, limits its scope, and supplies a reversal or cleanup instruction
