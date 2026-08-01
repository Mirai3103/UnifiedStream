# Automation guidelines

- Never commit or push directly to `main`.
- Before creating a commit, confirm the current branch is not `main`.
- Put every implementation change on a feature or fix branch and deliver it through a pull request targeting `main`.
- Do not merge until the required `openspec`, `rust`, `android`, and `frontend` checks pass.
- Do not disable or bypass branch protection to complete routine work.
- Follow `CONTRIBUTING.md` for local validation and the branch-to-pull-request workflow.
