# `strmbase` — DirectShow base classes (vendored)

The `CSource` / `CSourceStream` base classes `windows/dshow-camera/` derives its filter from. They shipped with the old DirectShow SDK, are not in the Windows SDK headers, and now live in Microsoft's samples repository.

## Provenance

| | |
| --- | --- |
| Upstream repository | <https://github.com/microsoft/Windows-classic-samples> |
| Upstream path | `Samples/Win7Samples/multimedia/directshow/baseclasses` |
| Commit taken | `d59e5f1dc9c768615e4e1ab1f0f009e6a3ed747c` |
| Licence | MIT — the upstream repository's `LICENSE`, copied verbatim beside these sources |
| Local modifications | None. The tree is byte-identical to upstream at that commit. |

Vendored rather than fetched at build time, per decision 1 of the change design: a build that reaches the network breaks when someone else's repository moves, and the point of pinning a licence is that the thing that was licence-checked is the thing that gets compiled.

The licence was established before these sources were read, as `repository-quality-gates` requires — the repository's own `LICENSE` (MIT) landed first, and MIT-against-MIT is the compatibility question this vendoring had to answer.

## Re-fetching and verifying

The tree can be reproduced exactly from the pinned commit:

```powershell
$sha = 'd59e5f1dc9c768615e4e1ab1f0f009e6a3ed747c'
$base = "https://raw.githubusercontent.com/microsoft/Windows-classic-samples/$sha/Samples/Win7Samples/multimedia/directshow/baseclasses"
# ...one Invoke-WebRequest per file in this directory, plus the repository LICENSE.
```

Should a local modification ever prove necessary, keep it as a patch file in this directory with a note here rather than as an edit in place, so the difference from upstream stays visible.

## Build notes

`windows/dshow-camera/strmbase.vcxproj` compiles these sources into a static library that both the filter and the conformance tool link. Two things about that project are deliberate.

- **`baseclasses.sln` and `baseclasses.vcproj` are unused.** They are Visual Studio 2008 project files, kept only so this directory matches upstream byte for byte. The MSBuild project beside the filter is what builds these sources.
- **The warning level is `/W3` with specific suppressions, and warnings are not errors here.** This is 1990s-era code compiled against a 2020s SDK; holding it to the `/W4 /WX` the filter's own code is held to would mean patching upstream, which is exactly what the provenance record above exists to avoid. The filter's own translation units get the strict settings.
