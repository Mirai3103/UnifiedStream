# UnifiedStream {{VERSION}} — Linux MVP

> [!WARNING]
> This is an experimental prerelease. The Android APK is debug-signed and the Linux packages are not code-signed. Verify `SHA256SUMS` before installation and use UnifiedStream only on a trusted LAN.

## Included artifacts

- Linux AppImage for broad distribution compatibility.
- Debian package for Debian and Ubuntu; build metadata and checksums are verified, but the Ubuntu runtime is not smoke-tested for this MVP.
- Universal Android debug APK for Android 7.0 and newer.
- SHA-256 checksum manifest covering all three installable artifacts.

## Install

Download all files into one directory and verify them:

```bash
sha256sum --check SHA256SUMS
```

Then follow the [Linux installation guide](https://github.com/Mirai3103/UnifiedStream/blob/{{TAG}}/docs/linux-installation.md) and [usage guide](https://github.com/Mirai3103/UnifiedStream/blob/{{TAG}}/docs/usage.md).

## System requirements

- Linux with PipeWire and WirePlumber.
- `v4l2loopback` for virtual camera output.
- FUSE 2 compatibility for AppImage, or Ubuntu 24.04 for the `.deb`.
- Android 7.0 or newer.
- A trusted LAN that permits mDNS, TCP 47810, and UDP 47811.

## MVP limitations

- Linux desktop only; Windows and macOS are not included.
- No stable signing, Play Store delivery, automatic update, Flatpak, or Snap.
- The APK is debug-signed and must be sideloaded.
- AppImage is the supported MVP path for CachyOS and Arch; no native Arch package is provided.
- Media quality can degrade on congested Wi-Fi, and the current codecs remain MJPEG and PCM.

## Known issues

- Some applications cache camera and audio device lists and must be restarted after a stream starts.
- Guest Wi-Fi, VPN routing, multicast filtering, and client isolation can prevent discovery or connection.
- If UDP 47811 is already occupied, the media socket can choose an ephemeral port that a strict firewall may block.

See [Troubleshooting](https://github.com/Mirai3103/UnifiedStream/blob/{{TAG}}/docs/troubleshooting.md) for diagnostics and safe recovery steps.
