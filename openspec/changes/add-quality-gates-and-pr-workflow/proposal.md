## Why

UnifiedStream currently relies on developers running checks manually, so regressions or invalid OpenSpec artifacts can reach `main`. The repository also needs an enforced review boundary: all future changes must arrive through pull requests instead of direct commits to `main`.

## What Changes

- Add continuous integration for Rust, Android, frontend, and OpenSpec validation.
- Define a stable set of required checks that must pass before merge.
- Require development on non-`main` branches and merge through pull requests.
- Configure GitHub protection for `main` to block direct pushes and require the CI checks.
- Document the local pre-PR workflow and the repository-admin setup needed for protection.

## Capabilities

### New Capabilities

- `repository-quality-gates`: Defines automated validation and the protected pull-request workflow for changes entering `main`.

### Modified Capabilities

None.

## Impact

- Adds GitHub Actions workflow configuration and contributor documentation.
- Runs existing Rust, Android, frontend, and OpenSpec commands in CI.
- Requires a GitHub branch protection rule or ruleset on `main`; this is repository configuration outside the codebase.
- Does not change application behavior, wire protocol, or runtime dependencies.
