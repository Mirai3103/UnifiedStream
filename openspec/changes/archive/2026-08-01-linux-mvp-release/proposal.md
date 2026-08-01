## Why

The Linux MVP already supports camera, microphone, and speaker streaming, but it lacks practical installation documentation, distributable packages, and a repeatable release process for users outside the development environment. This change turns the internal build into an experimental Linux release that users can install, operate, verify, and troubleshoot independently.

## What Changes

- Replace the template README with GitHub-ready documentation for installing, configuring, and using UnifiedStream on Linux with the Android companion app.
- Document system setup for `v4l2loopback`, PipeWire, firewall rules, mDNS, and device permissions.
- Standardize Linux desktop and Android APK packaging, including artifact names, versions, and checksums for an experimental release.
- Add a GitHub prerelease workflow with release notes, installable artifacts, and clearly stated Linux MVP limitations.
- Add concise, symptom-oriented troubleshooting for common installation, discovery, connection, virtual video, and virtual audio failures.

## Capabilities

### New Capabilities

- `linux-mvp-distribution`: Defines the user documentation, desktop and APK artifacts, prerelease contents, and troubleshooting required to distribute the Linux MVP.

### Modified Capabilities

None. This change does not alter the behavioral requirements of the existing media, discovery, transport, or session capabilities.

## Impact

- Repository-level documentation and the existing desktop README.
- Tauri bundle configuration, Android version metadata, and release build scripts or workflows where required.
- GitHub Actions and GitHub Release processes for desktop Linux packages, the Android APK, release notes, and checksums.
- Linux users must install distribution-specific dependencies and configure local network access and device permissions; runtime protocols and APIs remain unchanged.
