## Context

UnifiedStream already has a Tauri 2 Linux desktop app and a Gradle-based Android app, but the repository has no root README and `desktop/README.md` still contains template content. CI validates OpenSpec, Rust, Android, and frontend changes, but it does not produce distribution artifacts or a GitHub prerelease. Linux users currently have to infer how to install `v4l2loopback`, configure PipeWire and mDNS, open the firewall, and obtain access to `/dev/video*` devices.

This change crosses documentation, version metadata, desktop packaging, APK generation, and GitHub Actions. Its primary audiences are Linux users comfortable with basic command-line operations and maintainers producing experimental releases. Linux distributions differ in package managers, package names, firewall tools, and kernel module setup, so the documentation must distinguish verified commands from best-effort guidance.

## Goals / Non-Goals

**Goals:**

- Provide a clear path from the repository landing page to installation, system setup, connection, and use of all three media streams on Linux.
- Explain how to install, verify, and troubleshoot required system components without requiring users to inspect source code.
- Produce a consistently versioned experimental artifact set with recognizable names and downloadable checksums.
- Create a repeatable GitHub prerelease from a valid `main` revision, including MVP limitations and troubleshooting links.
- Keep documentation in GitHub Flavored Markdown with navigable headings, language-tagged code fences, and valid relative links.

**Non-Goals:**

- Windows, macOS, Play Store, Flatpak, Snap, or native packaging for every Linux distribution.
- Production code signing, automatic updates, or a stable production release channel.
- Changes to protocols, discovery, media codecs, or camera, microphone, and speaker behavior.
- Automatic firewall, kernel module, or system permission changes performed by the application.

## Decisions

### 1. Use the root README as the entry point and keep detailed guides in `docs/`

Create a repository-level `README.md` that introduces the project, identifies the Linux MVP status, provides a quick start, and links to focused setup and troubleshooting guides. Keep `desktop/README.md` limited to desktop development or redirect readers to the root documentation so there are not two competing user guides.

A single README containing every detail was considered, but it would be difficult to scan and maintain. An external wiki was also considered, but it would not be versioned with the source or reviewed in the same pull request.

### 2. Structure system documentation as install, configure, and verify

Each `v4l2loopback`, PipeWire, firewall, mDNS, and permissions section will explain its purpose, provide example commands for documented distributions, include verification steps, and link to related troubleshooting. Commands requiring elevated access must show `sudo` explicitly, and the guide must not recommend overly broad permissions such as `chmod 777`. Firewall rules must cover only the protocols and ports verified from the implementation and must be scoped to a trusted LAN.

Listing package names without verification steps was considered, but users would be unable to distinguish installation failures from runtime failures.

### 3. Publish an AppImage, Debian package, and experimental APK

The Linux desktop release will include an `.AppImage` for broad compatibility and a `.deb` package for Debian and Ubuntu. The Android release will include a directly installable experimental APK. If release-key infrastructure is not available, the artifact must use the debug variant, include `debug` in its filename, and be identified as non-production-signed in the release notes. RPM and Arch-native packages are out of scope; CachyOS and Arch users will use the AppImage for the MVP.

All artifacts will carry the same prerelease version where the build tools allow it and will be listed in `SHA256SUMS`. Publishing every target implied by `bundle.targets = "all"` was considered, but it would create an untested support matrix.

### 4. Create GitHub prereleases through a guarded workflow

The release workflow will publish only from a commit on `main` after the `openspec`, `rust`, `android`, and `frontend` quality gates pass for that revision. Tags will use SemVer prerelease syntax such as `v0.1.0-mvp.1`. A pinned Linux environment will build the packages, collect artifacts, generate checksums, and render release notes containing system requirements, installation guidance, limitations, known issues, and troubleshooting links.

Manual builds and uploads from a maintainer workstation were considered, but they are difficult to reproduce and can mix artifacts from different revisions. Automatic stable releases are inappropriate for an MVP.

### 5. Organize troubleshooting by visible symptom

Each troubleshooting entry will begin with the symptom a user observes, followed by a short diagnostic step, common causes, corrective action, and a confirmation step. The minimum scope covers failed mDNS discovery, blocked connectivity, missing or inaccessible virtual cameras, missing PipeWire nodes, denied Android permissions, and AppImage startup failures.

Organizing only by subsystem was considered, but it would require users to know which component is responsible before starting diagnosis.

## Risks / Trade-offs

- [Distribution commands and package names vary or change] → State the verified distribution and version, prefer package-manager-independent verification, and label other distributions as best effort.
- [AppImage still depends on host kernel features, FUSE, or graphics libraries] → Document runtime requirements and AppImage diagnostics while retaining `.deb` as an alternative for Debian and Ubuntu.
- [A debug APK can be mistaken for a trusted production build] → Include `debug` in the artifact name, warn in the README and release notes, and never label it production-ready.
- [Overly broad firewall rules increase network exposure] → Document only verified ports and protocols, scope examples to a trusted LAN, and include rule removal commands.
- [Tauri, Cargo, and Android versions can drift] → Add a release-time version consistency check and fail before upload when metadata does not match the tag.
- [Release workflow logic can diverge from CI] → Reuse the repository's existing commands, lockfiles, and pinned tool versions, with the quality workflow remaining the release prerequisite.

## Migration Plan

1. Add the root README and focused Linux setup and troubleshooting guides, verifying commands against source code and a clean Linux test environment or equivalent.
2. Standardize version metadata and bundle targets for AppImage, `.deb`, and the experimental APK.
3. Add the guarded prerelease workflow, release notes template, artifact naming rules, and checksum generation.
4. Run all quality gates, build every artifact, and smoke-test installation and startup on Linux with a physical Android device.
5. Merge only after the delta spec is synchronized, the change is archived, and all required checks pass; then monitor `main` and create the first prerelease from the verified merged revision.

Rollback removes a faulty GitHub prerelease and tag while preserving the merged source revision, then publishes a new SemVer prerelease after fixes. Published artifacts are never overwritten in place.

## Resolved Verification Decisions

- CachyOS is the Linux runtime smoke-test environment. Ubuntu runtime testing is explicitly excluded from this MVP verification; the `.deb` build, metadata, filename, and checksum remain verified.
- The MVP ships a clearly labeled debug-signed universal APK and does not require release-signing secrets.
- The desktop runtime constants are authoritative: TCP `47810` for control, UDP `47811` for media, and UDP `5353` multicast for mDNS discovery.
