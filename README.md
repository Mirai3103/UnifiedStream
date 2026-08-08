# UnifiedStream

Use an Android phone as a webcam, microphone, and wireless speaker for a Linux PC over a trusted local network.

> [!WARNING]
> UnifiedStream is an experimental Linux MVP. The prerelease APK uses Android debug signing and the desktop packages are not code-signed. Install only artifacts downloaded from this repository and verify their checksums.

## MVP features

- **Virtual camera:** Android camera to a webcam other applications can select. How that camera is presented is a platform choice: Linux writes frames to a `v4l2loopback` device the kernel offers, while a Windows build hands them to a filter each application loads for itself.
- **Virtual microphone:** Android microphone to a PipeWire source on Linux.
- **Wireless speaker:** PC system audio to the Android speaker or connected headphones. How that audio is obtained is a platform choice: Linux routes it through a PipeWire virtual sink it creates and makes the default output, while a Windows build captures the existing default output directly, creating no device and changing nothing the user selected.
- **Local discovery:** automatic mDNS discovery with a manual IP-address fallback.
- **Session controls:** pairing, reconnect, per-stream toggles, and live network telemetry.

macOS, Play Store distribution, stable signing, and automatic updates are not part of this release. Windows support is under way — the speaker works on a build made from source — but there is no Windows package, so this release ships Linux artifacts only.

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

No license has been declared yet. Until a license file is added, normal copyright restrictions apply.
