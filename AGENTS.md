# Automation guidelines

- Never commit or push directly to `main`.
- Before creating a commit, confirm the current branch is not `main`.
- Put every implementation change on a feature or fix branch and deliver it through a pull request targeting `main`.
- For an OpenSpec change, keep the implementation, delta-spec sync, and completed-change archive in the same pull request.
- Do not merge the implementation pull request until the code is complete, the specs are synced, the change is archived, and all required checks pass. Do not use a follow-up pull request just for sync or archive bookkeeping.
- Do not merge until the required `openspec`, `rust`, `android`, and `frontend` checks pass.
- After merging, monitor the `main` workflow and confirm all required jobs pass.
- Do not disable or bypass branch protection to complete routine work.
- Follow `CONTRIBUTING.md` for local validation and the branch-to-pull-request workflow.
