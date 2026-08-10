# Third-party notices

Components this product **redistributes** under a grant conditioned on attribution, identification, or notice, and where each obligation is verified.

This is not the same record as `windows/third_party/*/README.md`. Those cover source read or vendored into this repository, whose licence is satisfied by what the source tree contains. A component redistributed as a binary contributes nothing to the source tree: there is no file to carry a licence header, no compiler that sees it, and no test that exercises it. The obligation lives instead in what the product *displays*, which is the one place a change can silently remove it — deleting a line from an About screen breaks no build and fails no test, and converts a compliant product into an infringing one.

So every entry below names an automated check, and the check asserts against the rendered product rather than against a constant that feeds it.

## VB-CABLE Virtual Audio Device

| | |
| --- | --- |
| Component | VB-CABLE Virtual Audio Device (the standard, two-channel donationware edition) |
| Author | VB-Audio Software, Vincent Burel |
| Home | <https://vb-cable.com> |
| How it is distributed | A signed kernel-mode WDM driver and its installer, redistributed inside this product's Windows installer. Nothing is vendored, compiled, or linked; the desktop renders into the endpoint the driver creates. |
| Grant | VB-Audio's published terms permit redistribution of the standard donationware VB-CABLE, silent installation inside another installer explicitly included, without a licence fee. |
| Introduced by | The `add-windows-microphone` change (phase W4 of `docs/design.windows.md`). |

### What the grant requires

The grant is conditioned on attribution, and specifically on the end user being able to **identify the component and its author**, and being **in a position to donate for or license it** if they find it useful. Concretely, the shipped product must:

1. name the component — *VB-CABLE*;
2. name its author — *VB-Audio*;
3. describe it as **donationware**, the term the grant itself uses; and
4. carry a reachable link to <https://vb-cable.com>, where it can be donated for or licensed.

A bare "powered by VB-CABLE" line does not satisfy this. Naming the component without a means of acting on the notice fails requirement 4, and that is the part most easily lost in a redesign.

### Where it is satisfied, and what checks it

The notice is rendered in **Settings → About → Third-party components**, in `desktop/src/App.tsx`.

It is verified by `desktop/src/App.test.tsx`, in the `frontend` CI area, by two tests:

- one that renders the About surface and asserts all four requirements against the rendered DOM — the component, the author, the word *donationware*, and an anchor whose `href` reaches `vb-cable.com`;
- one that asserts the notice renders regardless of the platform state the backend reports, because a platform-conditional render is the one way the notice could disappear from a shipped build without the first test noticing.

Both fail if the notice is removed. The tests assert on rendered output rather than on a source constant deliberately: a notice that exists in the source and is never rendered satisfies nothing, and a component refactor is exactly how that happens.

### If the terms change

VB-Audio's terms are theirs to change, and this project's fallback is recorded in decisions 4 and 5 of `docs/design.windows.md`: write and sign a `sysvad`-derived driver of our own. `AudioSink` does not change either way, so the fallback is a packaging and driver problem rather than an architectural one.

Pinning and archiving the exact redistributed installer alongside each release — so an upstream change cannot retroactively alter what a shipped installer contains — is a W5 concern, recorded here because this is the record that would have to be re-checked against it.
