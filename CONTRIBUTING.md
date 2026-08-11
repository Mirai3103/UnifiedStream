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

# Windows native components, on a Windows workstation (see the note below)
msbuild windows\UnifiedStreamWindows.sln -p:Configuration=Release -p:Platform=x64
msbuild windows\UnifiedStreamWindows.sln -p:Configuration=Release -p:Platform=Win32

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

## Windows native prerequisites

`windows/` holds the DirectShow camera filter and its conformance tool. It is C++ built by MSBuild,
not by Cargo, so it is its own quality area with its own required check and its own toolchain:

- **Visual Studio 2022** (or the standalone Build Tools) with the **Desktop development with C++**
  workload, which supplies MSBuild and the MSVC compiler.
- The **Windows 10/11 SDK**, installed by that workload. The DirectShow base classes the filter
  derives from are *not* in the SDK — they are vendored in `windows/third_party/strmbase/` and built
  from source; see the README beside them.

Build both architectures from a Developer Command Prompt or any shell with MSBuild on `PATH`. Both
matter: a 32-bit application can only load a 32-bit filter, and the desktop reports a filter
registered for one architecture as unusable rather than as installed.

The cross-toolchain conformance test runs the C++ consumer against the real Rust producer over the
real named section. Start the reader first, because it attaches to a section the publisher creates:

```powershell
$reader = Start-Process -FilePath 'windows\x64\Release\ring_conform.exe' -PassThru -NoNewWindow
Push-Location desktop/src-tauri
cargo run -p unifiedstream-video --example ring_smoke --release -- --publish-only
Pop-Location
$reader.WaitForExit(); $reader.ExitCode
```

It exits non-zero if any frame was accepted with wrong contents, and if no frame was accepted at
all. This is the only test in either build that observes both halves of the frame transport at
once, so it is the one to run after touching either.

`filter_conform` is the other half of the story, and needs no producer and no elevation:

```powershell
windows\x64\Release\filter_conform.exe windows\x64\Release\UnifiedStreamCamera64.dll
windows\Win32\Release\filter_conform.exe windows\Win32\Release\UnifiedStreamCamera32.dll
```

It checks the COM surface a capture application touches before it ever asks for a frame — the pin
category, the capability list, and format selection — entirely from outside, through COM. Run it
after touching anything in `filter.cpp`. A filter can deliver perfect frames into a graph you build
yourself and still be unopenable by every real application; that is not hypothetical, it is why this
program exists.

Registering the filter is not part of building it. `regsvr32` from an elevated prompt, one
architecture at a time — the desktop's setup hint names the exact command for whichever half is
missing.

### VB-CABLE, for the microphone

The Windows microphone renders into VB-CABLE's playback device rather than creating one, so nothing
in this repository builds it and nothing installs it yet — bundling it inside the installer is W5.
Until then it is a **manual prerequisite for running the microphone locally**, and its absence is
why the Microphone card refuses on a fresh development machine:

1. Install VB-CABLE (donationware, by VB-Audio) from <https://vb-cable.com>. It is a signed
   kernel-mode driver; the installer wants elevation and a reboot.
2. Confirm both halves of the cable appear — `CABLE Input` under playback, `CABLE Output` under
   recording.
3. Leave your system output on a real playback device. Pointing it at `CABLE Input` is a supported
   configuration that produces a feedback loop, which is why the Speaker stream refuses while it
   holds.

The microphone can then be exercised without a phone, which is also how the endpoint discrimination
is measured on a machine:

```powershell
cargo run -p unifiedstream-audio --example wasapi_mic
```

It resolves the destination, logs every VB-Audio playback device it considered and why each was
accepted or rejected, and plays a tone for five seconds — select `CABLE Output` in any recording
application to hear it. CI cannot cover any of this: the Windows runners have no audio device.

Obligations that come with redistributing VB-CABLE, and the check that keeps them satisfied, are in
[docs/third-party-notices.md](docs/third-party-notices.md).

Record the commands you ran in the pull-request template. Explain any relevant check you could not run locally.

## Open and merge the pull request

Push the branch and open a pull request targeting `main`:

```sh
git push --set-upstream origin HEAD
gh pr create --base main --fill
```

The GitHub web interface may be used instead of `gh`. Link the OpenSpec change when applicable, keep the pull request scoped, and address review feedback on the same branch.

Merge only through GitHub after the required `openspec`, `rust (ubuntu-24.04)`, `rust (windows-latest)`, `windows-native`, `android`, and `frontend` checks pass. Do not bypass branch protection for normal development.

The `main` ruleset lists those six checks by name. Adding a quality area to `.github/workflows/quality.yml` means adding its check to the ruleset in the same change, or the area runs without gating anything.
