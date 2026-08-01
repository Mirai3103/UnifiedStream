## Context

The repository contains four independently validated areas: Rust/Tauri, Android/Kotlin, React/TypeScript, and OpenSpec planning artifacts. All checks are currently manual, `.github/workflows` does not exist, and `main` accepts direct pushes. The GitHub remote is `Mirai3103/UnifiedStream`.

Repository files can define CI and document the workflow, but only GitHub repository settings can actually reject a direct push. The implementation therefore has a code portion and a repository-administration portion.

## Goals / Non-Goals

**Goals:**

- Run deterministic quality checks for every pull request targeting `main`.
- Keep independent checks parallel and make failures easy to identify.
- Prevent direct pushes, force pushes, and deletion of `main`.
- Require successful checks before a pull request can merge.
- Give contributors a short, repeatable branch-to-PR workflow.

**Non-Goals:**

- Building release installers or publishing artifacts.
- Adding deployment, release, or device/instrumentation-test pipelines.
- Requiring another person's approval; the project may currently have one maintainer.
- Changing application behavior or dependencies used at runtime.

## Decisions

### D1: GitHub Actions is the CI provider

Add one quality workflow triggered by pull requests targeting `main`, pushes to `main`, and manual dispatch. Pull-request checks prevent bad merges; the push trigger verifies the resulting branch state; manual dispatch supports diagnostics.

Alternative considered: local Git hooks. Hooks are useful but can be skipped and cannot enforce repository policy, so they are not the quality boundary.

### D2: Use four parallel required jobs

The workflow exposes stable job names for `openspec`, `rust`, `android`, and `frontend`:

- `openspec`: install a pinned OpenSpec CLI and run `openspec validate --all`.
- `rust`: install Linux build dependencies, then run formatting, Clippy with warnings denied for workspace production targets, and workspace tests.
- `android`: use JDK 17 and run debug unit tests plus debug APK assembly through the checked-in Gradle wrapper.
- `frontend`: use the committed Bun lockfile with a pinned Bun version, install with the frozen lockfile, and run the production build.

Separate jobs make failures attributable and allow GitHub to require each check independently. Dependency caches may reduce runtime but must not replace lockfiles or checked-in wrappers.

Alternative considered: one sequential job. It is simpler, but slower and hides later failures after the first command stops.

### D3: A GitHub ruleset protects `main`

Configure a ruleset targeting `main` that requires a pull request and all four stable CI checks, blocks force pushes and branch deletion, and applies to repository administrators. Required approving reviews remain zero initially so a solo maintainer can merge their own pull request after checks pass. An emergency bypass, if GitHub requires one, must be narrowly scoped and documented rather than used for normal development.

Alternative considered: documenting “do not push to main.” Documentation alone cannot enforce the rule.

### D4: Document the contributor workflow in the repository

Add concise contribution instructions: branch from updated `main`, make commits on the branch, run relevant local checks, push the branch, open a pull request, wait for required checks, and merge through GitHub. A pull-request template provides a checklist without duplicating the full CI commands.

## Risks / Trade-offs

- [The first CI run may expose environment assumptions or existing Clippy warnings] → Fix the baseline in this change before making checks required.
- [GitHub job names changed later can invalidate required-check configuration] → Treat the four job names as a stable interface and document them in the setup steps.
- [External actions or tool releases change behavior] → Pin tool versions and use committed lockfiles/wrappers.
- [Branch protection cannot be represented fully in Git] → Include a verified repository-admin task and document the expected ruleset settings.
- [Strict protection can block emergency maintenance] → Prefer reverting through a pull request; document any narrowly scoped emergency bypass and audit its use.

## Migration Plan

1. Fix current warnings and make every planned command pass locally.
2. Add the workflow and contribution documentation on a feature branch.
3. Open a pull request and verify all four checks on GitHub.
4. Configure the `main` ruleset using the observed stable check names.
5. Inspect the active ruleset through GitHub settings/API and verify required-check merge gating with a harmless test pull request; do not probe protection with a real direct push.

Rollback: disable the ruleset temporarily if CI infrastructure itself prevents urgent work; keep changes on a branch and restore protection immediately after correcting the workflow.

## Open Questions

- None blocking. Required human approvals can be increased from zero when another regular maintainer is available.
