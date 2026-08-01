## 1. Establish a clean quality baseline

- [x] 1.1 Create a feature branch for this change and confirm no implementation commit or push targets `main` directly.
- [x] 1.2 Fix the existing production Clippy warnings, then verify Rust formatting, warning-denied Clippy, and all workspace tests pass.
- [x] 1.3 Verify the Android debug unit tests and debug APK assembly pass through `android/gradlew` with JDK 17.
- [x] 1.4 Verify the frontend installs from the committed `bun.lock` with a frozen lockfile and completes its production build.
- [x] 1.5 Verify `openspec validate --all` passes with OpenSpec CLI 1.6.0.

## 2. Add continuous integration

- [x] 2.1 Add a least-privilege GitHub Actions workflow for pull requests to `main`, pushes to `main`, and manual dispatch, with concurrency that cancels superseded runs.
- [x] 2.2 Add the stable `openspec` job using a pinned OpenSpec CLI and `openspec validate --all`.
- [x] 2.3 Add the stable `rust` job with cached dependencies, required Linux system packages, formatting, warning-denied production Clippy, and workspace tests.
- [x] 2.4 Add the stable `android` job with JDK 17, Gradle caching, debug unit tests, and debug APK assembly.
- [x] 2.5 Add the stable `frontend` job with pinned Bun, frozen-lockfile installation, and the production build.

## 3. Document the pull-request workflow

- [x] 3.1 Add concise contributor instructions for branch creation, local validation, pushing the branch, opening a pull request, and merging only after required checks pass.
- [x] 3.2 Add repository guidance for automation/agents that forbids committing or pushing directly to `main`.
- [x] 3.3 Add a pull-request template covering scope, OpenSpec change linkage, tests run, and relevant manual verification.

## 4. Verify CI and protect main

- [x] 4.1 Push the feature branch and open a pull request for this change without committing directly to `main`.
- [x] 4.2 Confirm the `openspec`, `rust`, `android`, and `frontend` checks all run independently and pass on the pull request.
- [x] 4.3 Configure a GitHub ruleset for `main` that requires pull requests and the four checks, applies to administrators, and blocks force pushes and deletion; keep required approvals at zero until another maintainer is available.
- [x] 4.4 Inspect the active ruleset through GitHub settings or API and verify merge gating with the pull request, without testing protection through a real direct push.
- [x] 4.5 Merge through the pull request flow and confirm the post-merge `main` workflow passes.
