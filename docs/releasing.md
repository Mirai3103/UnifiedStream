# Creating an experimental release

This runbook is for maintainers. Releases are created only after the implementation, delta-spec sync, and completed-change archive are in one pull request, all required checks pass, the pull request is merged, and the resulting `main` workflow succeeds.

## Version policy

The Git tag is the authoritative prerelease version and uses SemVer prerelease syntax, for example `v0.1.0-mvp.1`. The tag's base version (`0.1.0`) must match all committed metadata:

- `desktop/src-tauri/tauri.conf.json`
- `desktop/src-tauri/Cargo.toml` workspace package version
- `desktop/package.json`
- `android/app/build.gradle.kts` `versionName`

The complete prerelease version appears in artifact filenames and GitHub release metadata. The embedded application metadata uses the matching base version because Android `versionCode` remains the install-order authority for sideloaded MVP builds.

Check the version before a build:

```bash
scripts/check-release-version.sh v0.1.0-mvp.1
```

## Build locally

Install the same Linux, Bun, Rust, JDK, and Android SDK dependencies used by CI, then run:

```bash
scripts/build-release-artifacts.sh v0.1.0-mvp.1 release-dist
```

The output directory contains one AppImage, one `.deb`, one explicitly named debug APK, and `SHA256SUMS`. The script refuses to overwrite existing release output and verifies the checksum manifest before returning success.

Use the [Linux MVP smoke-test record](release-smoke-test.md) as the pull-request template for environments, commands, results, skipped checks, and known limitations. Replace its local evidence with results from the candidate revision when producing a later prerelease.

## Publish through GitHub Actions

1. Confirm the pull request is merged and all `openspec`, `rust`, `android`, and `frontend` jobs passed on the resulting `main` revision.
2. Open **Actions → Linux MVP Prerelease → Run workflow**.
3. Select the `main` branch and enter an unused tag such as `v0.1.0-mvp.1`.
4. Wait for the workflow to verify the exact revision, rebuild the artifacts, verify checksums, and create a GitHub prerelease.
5. Download the published artifacts into a clean directory and run `sha256sum --check SHA256SUMS`.
6. Install the published files and repeat the release smoke test rather than relying only on workflow outputs.

The workflow refuses a non-`main` dispatch, invalid or mismatched version, unsuccessful Quality workflow, existing Git tag, or existing GitHub release. It creates the tag only when publishing the prerelease.

## Roll back a faulty prerelease

First mark the release notes as withdrawn so users understand the state. Then remove the prerelease and its tag through the GitHub UI, or use the following explicit tag after confirming it is the intended prerelease:

```bash
gh release delete v0.1.0-mvp.1 --cleanup-tag
```

Do not reuse or overwrite a published version. Fix the problem through a new pull request, wait for all checks on `main`, and publish a new tag such as `v0.1.0-mvp.2`.

## Post-merge monitoring

After every merge, open the Quality workflow for `main` and confirm all required jobs complete successfully. Do not invoke the prerelease workflow while any required job is incomplete, cancelled, or failing. Record the workflow URL, release URL, tested distributions, Android device, and checksum result in the release notes or maintainer log.
