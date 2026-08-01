## 1. Confirm Release Inputs

- [x] 1.1 Trace the runtime discovery, control, and media port constants and document the exact TCP/UDP and mDNS traffic required by the firewall guide.
- [x] 1.2 Verify the supported `v4l2loopback`, PipeWire, mDNS, FUSE, and device-permission setup on CachyOS, and document the Debian package as runtime-unverified for this MVP.
- [x] 1.3 Decide whether the MVP APK will use explicit debug signing or repository secrets for release signing, and record the decision in user and release documentation.
- [x] 1.4 Define one prerelease version source and verify how it maps to Tauri, Cargo workspace, Android `versionName`, artifact filenames, and SemVer tags.

## 2. Write User Documentation

- [x] 2.1 Create a GitHub Flavored Markdown root `README.md` with project status, supported streams, prerequisites, a concise quick start, MVP limitations, and relative links to detailed guides.
- [x] 2.2 Create the Linux installation guide with AppImage and `.deb` installation, APK sideloading, same-LAN preparation, and checksum verification.
- [x] 2.3 Document `v4l2loopback` installation, module configuration, `/dev/video*` verification, least-privilege access, persistence, and rollback for the verified distributions.
- [x] 2.4 Document PipeWire service checks, UnifiedStream virtual source and sink verification, and safe recovery commands.
- [x] 2.5 Document verified firewall ports and protocols, trusted-LAN scoping, mDNS verification, manual-address fallback, and commands to remove added rules.
- [x] 2.6 Add an ordered end-to-end usage guide for connecting both apps, granting Android permissions, enabling each media stream, and verifying the virtual devices in another Linux application.
- [x] 2.7 Replace the template desktop README with focused development information and a link to the canonical root user documentation.

## 3. Add Troubleshooting

- [x] 3.1 Add symptom-oriented entries for failed mDNS discovery and firewall-blocked LAN connectivity, including diagnostics, fixes, confirmation, and rollback.
- [x] 3.2 Add entries for a missing, busy, or inaccessible `v4l2loopback` device and for applications caching an obsolete virtual camera.
- [x] 3.3 Add entries for missing PipeWire nodes, inactive user services, and audio routing failures.
- [x] 3.4 Add entries for denied Android camera, microphone, notification, or local-network permissions and explain how to re-grant them safely.
- [x] 3.5 Add entries for AppImage startup failures, including executable permission, FUSE, WebKit, and log collection checks.

## 4. Standardize Release Artifacts

- [x] 4.1 Restrict the Tauri Linux bundle configuration to the supported AppImage and Debian targets and replace placeholder package metadata required by those bundles.
- [x] 4.2 Align desktop and Android version metadata with the chosen prerelease version policy and add a check that rejects tag-to-metadata mismatches.
- [x] 4.3 Add reproducible release commands that build the AppImage, `.deb`, and selected APK variant from committed sources and lockfiles.
- [x] 4.4 Normalize artifact filenames to include project, version, platform or architecture, package type, and APK debug status where applicable.
- [x] 4.5 Generate `SHA256SUMS` from the final renamed artifacts and verify the manifest before upload.

## 5. Automate the Experimental Release

- [x] 5.1 Add a guarded GitHub Actions prerelease workflow that accepts or derives a SemVer prerelease tag and resolves the exact selected `main` revision.
- [x] 5.2 Make the workflow stop unless the `openspec`, `rust`, `android`, and `frontend` checks succeeded for that exact revision.
- [x] 5.3 Build desktop and Android artifacts in pinned environments, transfer them without rebuilding between jobs, and attach only artifacts from the selected revision.
- [x] 5.4 Add a release notes template covering requirements, installation, checksums, APK signing status, MVP limitations, known issues, and documentation links.
- [x] 5.5 Publish the output as a GitHub prerelease without overwriting an existing tag or release, and document the post-merge invocation and rollback procedure.

## 6. Verify the Release Path

- [x] 6.1 Validate GitHub Markdown structure, language-tagged code fences, heading hierarchy, and every relative documentation link.
- [x] 6.2 Run `openspec validate --all` and the complete Rust, Android, and frontend validation commands required by `CONTRIBUTING.md`.
- [x] 6.3 Build the full artifact set locally or in a workflow dry run and verify filenames, embedded versions, APK signing status, and `SHA256SUMS`.
- [x] 6.4 Install and launch the AppImage on CachyOS, verify the `.deb` build metadata without Ubuntu runtime testing, and sideload the APK on a physical Android device.
- [x] 6.5 Follow the published quick start as a user, verify camera, microphone, and speaker output, and exercise at least one recovery path for discovery, video, and audio.
- [x] 6.6 Record smoke-test environments, commands, results, known limitations, and any unavailable local checks in the pull request.

## 7. Complete the OpenSpec Change

- [x] 7.1 Reconcile implementation behavior and release documentation with the `linux-mvp-distribution` delta spec after verification.
- [x] 7.2 Sync the delta spec to the main specs and archive the completed change in the same implementation pull request before merge.
- [x] 7.3 Confirm all required pull-request checks pass before merge and document the post-merge `main` workflow monitoring and first-prerelease steps for the maintainer.
