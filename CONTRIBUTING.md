# Contributing to UnifiedStream

All changes enter `main` through a pull request. Never commit or push directly to `main`.

## Prepare a branch

Start from an up-to-date `main`, then create a focused branch:

```sh
git switch main
git pull --ff-only origin main
git switch -c <type>/<short-description>
```

Commit changes only on that branch. If the work has an OpenSpec change, use its name in the branch or pull-request description.

## Validate locally

Run the checks relevant to your change. Before requesting merge, run the full set when your environment supports it:

```sh
# OpenSpec (requires OpenSpec CLI 1.6.0)
openspec validate --all

# Rust/Tauri production targets and workspace tests
(cd desktop/src-tauri && cargo fmt --all -- --check)
(cd desktop/src-tauri && cargo clippy --workspace --lib --bins -- -D warnings)
(cd desktop/src-tauri && cargo test --workspace)

# Non-Linux compilation and tests, on a Windows workstation (see the note below)
(cd desktop/src-tauri && cargo check --workspace)
(cd desktop/src-tauri && cargo clippy --workspace --lib --bins -- -D warnings)
(cd desktop/src-tauri && cargo test --workspace)

# Android (requires JDK 17)
(cd android && ./gradlew testDebugUnitTest assembleDebug)

# Frontend (requires Bun 1.3.14)
(cd desktop && bun install --frozen-lockfile && bun run build)
```

The Rust area is verified on two platforms and reports one check per platform, so a Linux
workstation cannot run the whole set. `cargo check` and Clippy for a non-Linux target are
expected to be unrunnable there: `tauri-build` does host-side work for a Windows target, so
cross-checking from Linux fails for toolchain reasons rather than code reasons. Leave that leg
to CI's `rust (windows-latest)` check, or run it on a Windows machine.

`cargo test --workspace` runs everywhere, including on a Windows workstation and on the Windows
CI leg. The suite is portable — the integration tests exercise the transport and the shared
pipeline rather than the system — and a test that needs an audio or video device would break
that, since the Windows runners have neither. Code that genuinely needs a real device is
verified by a manual smoke run instead.

Record the commands you ran in the pull-request template. Explain any relevant check you could not run locally.

## Open and merge the pull request

Push the branch and open a pull request targeting `main`:

```sh
git push --set-upstream origin HEAD
gh pr create --base main --fill
```

The GitHub web interface may be used instead of `gh`. Link the OpenSpec change when applicable, keep the pull request scoped, and address review feedback on the same branch.

Merge only through GitHub after the required `openspec`, `rust (ubuntu-24.04)`, `rust (windows-latest)`, `android`, and `frontend` checks pass. Do not bypass branch protection for normal development.
