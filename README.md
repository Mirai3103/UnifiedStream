# UnifiedStream

Use an Android phone as a webcam, microphone, and wireless speaker for a Linux PC over a trusted local network.

> [!WARNING]
> UnifiedStream is an experimental Linux MVP. The prerelease APK uses Android debug signing and the desktop packages are not code-signed. Install only artifacts downloaded from this repository and verify their checksums.

## MVP features

- **Virtual camera:** Android camera to a webcam other applications can select. How that camera is presented is a platform choice: Linux writes frames to a `v4l2loopback` device the kernel offers, while a Windows build publishes them into shared memory that a DirectShow filter — registered once with `regsvr32`, for both application architectures — reads inside each application's own process. Windows coverage is broad but not universal: Zoom, Discord, OBS, Skype, and Chromium-based browsers see the camera; UWP, Store, and Media Foundation-only applications do not. See the [usage guide](docs/usage.md#on-a-windows-build).
- **Virtual microphone:** Android microphone to an input device other applications can select. How that input is presented is a platform choice: Linux creates a PipeWire source named **UnifiedStream Microphone** that exists only while the stream does, while a Windows build renders into VB-CABLE — donationware by VB-Audio, redistributed with the application — and applications select **CABLE Output (VB-Audio Virtual Cable)**, which is present whether or not a stream is running. See the [usage guide](docs/usage.md#on-a-windows-build-1).
- **Wireless speaker:** PC system audio to the Android speaker or connected headphones. How that audio is obtained is a platform choice: Linux routes it through a PipeWire virtual sink it creates and makes the default output, while a Windows build captures the existing default output directly, creating no device and changing nothing the user selected.
- **Local discovery:** automatic mDNS discovery with a manual IP-address fallback.
- **Session controls:** pairing, reconnect, per-stream toggles, and live network telemetry.

macOS, Play Store distribution, stable signing, and automatic updates are not part of this release. Windows support is under way — the speaker and the camera both work on a build made from source — but there is no Windows package, so this release ships Linux artifacts only.

## Requirements

- A Linux PC running PipeWire and WirePlumber.
- An Android 7.0 or newer device.
- Both devices on the same trusted LAN without client isolation.
- `v4l2loopback` for the virtual camera.
- An AppImage-capable system or Debian/Ubuntu for the `.deb` package.

See [Linux installation and system setup](docs/linux-installation.md) for distribution-specific commands.

## Quick start

1. Download the AppImage or `.deb`, Android debug APK, and `SHA256SUMS` from the same GitHub prerelease.
2. Verify the downloads:

   ```bash
   sha256sum --check SHA256SUMS
   ```

3. Complete the [`v4l2loopback`, PipeWire, mDNS, firewall, and permissions setup](docs/linux-installation.md#system-setup).
4. Install the desktop package and sideload the APK.
5. Put both devices on the same trusted LAN and launch UnifiedStream on the PC first.
6. Open the Android app, select the discovered PC, and approve the pairing request on the desktop.
7. Enable Camera, Microphone, or Speaker independently and select the resulting device in the Linux application where it will be used.

For the complete flow and verification steps, read the [usage guide](docs/usage.md).

## Network ports

| Traffic | Direction at the Linux PC | Port | Purpose |
| --- | --- | --- | --- |
| UDP multicast | Inbound and outbound | 5353 | mDNS/DNS-SD discovery |
| TCP | Inbound | 47810 | Pairing and session control |
| UDP | Inbound and outbound | 47811 preferred | Camera, microphone, and speaker media |

The media socket can fall back to an ephemeral port if UDP 47811 is already occupied; the peers exchange the actual port during the handshake. See the [firewall guidance](docs/linux-installation.md#firewall) before enabling a restrictive firewall.

## Documentation

- [Linux installation and system setup](docs/linux-installation.md)
- [Using camera, microphone, and speaker](docs/usage.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Creating an experimental release](docs/releasing.md)
- [Contributing](CONTRIBUTING.md)

## Security and privacy

UnifiedStream is designed for a trusted LAN. Pair only with devices you recognize, do not expose its ports to the public internet, and scope firewall rules to the local subnet. Camera and microphone indicators remain visible on Android while those streams are active.

## Development

Follow [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, validation commands, branches, and pull requests. Component-specific desktop commands are in [desktop/README.md](desktop/README.md).

The desktop application layer depends only on media traits, and each integration crate selects its platform implementation in one module. A target without an implementation compiles and runs with its media toggles reporting the feature as unavailable, and CI verifies that on a Windows runner every pull request. [Windows architecture decisions](docs/design.windows.md) records the decisions that seam is shaped to accommodate.

## License

UnifiedStream is released under the [MIT License](LICENSE), which covers the repository as a whole.

Third-party source vendored into this repository keeps its own licence, recorded beside it. `windows/third_party/strmbase/` holds the DirectShow base classes from `microsoft/Windows-classic-samples`, which are MIT.
