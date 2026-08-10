## ADDED Requirements

### Requirement: Redistributed third-party components carry their required notices

Where the product redistributes a third-party component under a grant conditioned on attribution, identification, or notice, the repository SHALL record what that grant requires and SHALL verify by automated check that the shipped product still satisfies it.

This is distinct from recording provenance for source that is read or vendored, and is not covered by it. A component redistributed as a binary contributes nothing to the source tree: there is no file to record a licence header against, no compiler that sees it, and no test that exercises it. The obligation lives instead in what the product displays to its user, which is the one place a change can silently remove it — deleting a line from an About screen breaks no build, fails no test, and converts a compliant product into an infringing one.

The check SHALL fail when the required notice is absent, and MUST NOT be satisfied by the notice existing only in documentation, in a design record, or in a comment. Where the grant requires that the user be able to act on the notice — to identify the component's author, or to reach the place it may be paid for — the check SHALL verify that the means of acting is present, not merely that the component is named.

Recording the requirement SHALL happen as part of the change that introduces the dependency, for the same reason provenance is established before source is read: an obligation discovered after distribution has already been breached.

#### Scenario: A component is redistributed under a notice obligation

- **WHEN** a change introduces a third-party component that the product redistributes under a grant requiring attribution or notice
- **THEN** what the grant requires is recorded in the repository as part of that change
- **AND** an automated check verifies the shipped product satisfies it

#### Scenario: A required notice is removed

- **WHEN** a change removes or empties a required notice from the product's user-facing surface
- **THEN** the check fails and the merge is blocked
- **AND** the failure identifies which component's obligation is unmet

#### Scenario: A notice names the component but cannot be acted on

- **WHEN** the grant requires the user be able to identify the component's author or reach where it may be paid for, and the product names the component without either
- **THEN** the check fails
- **AND** naming the component alone does not satisfy it

#### Scenario: The obligation is recorded only in a design document

- **WHEN** a redistributed component's notice obligation is described in a design record but no automated check verifies it
- **THEN** the obligation is treated as unverified
- **AND** the change that introduced the component is incomplete
