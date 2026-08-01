# Linux installation and system setup

This guide prepares a Linux PC and Android phone for the UnifiedStream experimental MVP. Commands and the AppImage runtime were verified against CachyOS with the `linux-cachyos` kernel. Ubuntu 24.04 commands and the Debian package are provided for convenience, but the `.deb` runtime was not smoke-tested for this MVP. Closely related distributions are supported on a best-effort basis.

## Contents

- [Download and verify](#download-and-verify)
- [Install the desktop app](#install-the-desktop-app)
- [Install the Android app](#install-the-android-app)
- [System setup](#system-setup)
- [`v4l2loopback`](#v4l2loopback)
- [PipeWire](#pipewire)
- [mDNS](#mdns)
- [Firewall](#firewall)
- [Device permissions](#device-permissions)
- [Remove the configuration](#remove-the-configuration)

## Download and verify

Download these files from one GitHub prerelease:

- `UnifiedStream-<version>-linux-<architecture>.AppImage`
- `UnifiedStream-<version>-linux-<architecture>.deb`
- `UnifiedStream-<version>-android-universal-debug.apk`
- `SHA256SUMS`

The APK is debug-signed and intended only for MVP testing. Check every downloaded artifact before installation:

```bash
sha256sum --check SHA256SUMS
```

An unchanged file prints `OK`. Do not install a file that is missing from the manifest or reports `FAILED`.

## Install the desktop app

### AppImage on CachyOS, Arch, and other distributions

Install the FUSE 2 compatibility library when the AppImage requires it:

```bash
# CachyOS and Arch
sudo pacman -S fuse2

# Ubuntu 24.04
sudo apt update
sudo apt install libfuse2t64
```

Make the downloaded file executable and run it:

```bash
chmod u+x UnifiedStream-*-linux-*.AppImage
./UnifiedStream-*-linux-*.AppImage
```

If FUSE is unavailable, use the AppImage runtime's extraction fallback:

```bash
./UnifiedStream-*-linux-*.AppImage --appimage-extract-and-run
```

### Debian package on Ubuntu 24.04

Install the local package with APT so dependencies are resolved:

```bash
sudo apt install ./UnifiedStream-*-linux-*.deb
```

Remove it later with:

```bash
sudo apt remove unified-stream
```

## Install the Android app

The MVP APK uses Android debug signing and is not a Play Store or production-signed build.

1. Transfer `UnifiedStream-<version>-android-universal-debug.apk` to the phone.
2. Open it from the phone's file manager.
3. If prompted, allow that file-manager app to **Install unknown apps**.
4. Install UnifiedStream, then disable that installer permission again if it is no longer needed.

Developers with Android platform tools can install it over USB instead:

```bash
adb install -r UnifiedStream-*-android-universal-debug.apk
```

## System setup

UnifiedStream does not modify kernel modules, firewall rules, services, or permissions automatically. Complete the following sections before testing all three streams.

## `v4l2loopback`

The camera stream writes decoded frames to a virtual V4L2 camera named `UnifiedStream Camera`.

### Install

The current CachyOS `linux-cachyos` kernel already includes the module. Check before installing anything:

```bash
modinfo v4l2loopback
```

On Arch-derived kernels that do not include it, install the matching kernel headers and DKMS package:

```bash
sudo pacman -S v4l2loopback-dkms v4l-utils
```

On Ubuntu 24.04:

```bash
sudo apt update
sudo apt install "linux-headers-$(uname -r)" v4l2loopback-dkms v4l-utils
```

### Configure and verify

Load one capture device with exclusive capabilities so conferencing applications recognize it correctly:

```bash
sudo modprobe v4l2loopback devices=1 video_nr=42 card_label="UnifiedStream Camera" exclusive_caps=1
v4l2-ctl --list-devices
v4l2-ctl --device=/dev/video42 --all
```

The desktop backend searches for a loopback output device and does not require video number 42 specifically. If that number is occupied, omit `video_nr=42` and use the `/dev/video*` path printed by `v4l2-ctl --list-devices`.

To load the same device after boot:

```bash
printf '%s\n' v4l2loopback | sudo tee /etc/modules-load.d/unifiedstream.conf
printf '%s\n' 'options v4l2loopback devices=1 video_nr=42 card_label="UnifiedStream Camera" exclusive_caps=1' | sudo tee /etc/modprobe.d/unifiedstream.conf
```

Reboot, or unload and reload the module when no application is using it:

```bash
sudo modprobe -r v4l2loopback
sudo modprobe v4l2loopback
```

### Roll back

```bash
sudo rm /etc/modules-load.d/unifiedstream.conf
sudo rm /etc/modprobe.d/unifiedstream.conf
sudo modprobe -r v4l2loopback
```

The files above are dedicated to UnifiedStream; inspect them before removal if they were edited for other applications.

## PipeWire

UnifiedStream creates runtime PipeWire nodes named `UnifiedStream Microphone` and `UnifiedStream Speaker`; they exist only while their streams are active.

Install and start the user services:

```bash
# CachyOS and Arch
sudo pacman -S pipewire pipewire-pulse wireplumber

# Ubuntu 24.04
sudo apt update
sudo apt install pipewire pipewire-pulse wireplumber

systemctl --user enable --now pipewire pipewire-pulse wireplumber
systemctl --user --no-pager --full status pipewire pipewire-pulse wireplumber
```

After connecting the phone and enabling a stream, verify the nodes:

```bash
wpctl status
pw-cli list-objects Node | sed -n '/UnifiedStream/,+8p'
```

Restart the user audio stack safely if it becomes stale:

```bash
systemctl --user restart wireplumber pipewire pipewire-pulse
```

This interrupts current audio applications; reopen them after the services recover. PipeWire setup is session-level and adds no persistent UnifiedStream node to remove.

## mDNS

The desktop advertises `_unifiedstream._udp.local.` and Android browses for it. Both devices must be on the same LAN, multicast must be enabled, and guest Wi-Fi client isolation must be disabled.

Install and enable Avahi on Linux:

```bash
# CachyOS and Arch
sudo pacman -S avahi nss-mdns

# Ubuntu 24.04
sudo apt update
sudo apt install avahi-daemon avahi-utils libnss-mdns

sudo systemctl enable --now avahi-daemon
systemctl --no-pager --full status avahi-daemon
```

Launch the desktop app, then verify its advertisement:

```bash
avahi-browse --resolve --terminate _unifiedstream._udp
```

If Android still finds nothing after ten seconds, use **Connect by address** with the PC's LAN IP and control port `47810`. Manual entry bypasses discovery, not firewall or client isolation.

Disable Avahi only if no other local-discovery applications need it:

```bash
sudo systemctl disable --now avahi-daemon
```

## Firewall

The authoritative defaults in both implementations are:

- UDP 5353 multicast for mDNS/DNS-SD.
- TCP 47810 inbound to the Linux PC for pairing and control.
- UDP 47811 in both directions for media by default.

If UDP 47811 is occupied, UnifiedStream binds an ephemeral media port and exchanges it during the handshake. A strict fixed-port firewall will block that fallback, so keep 47811 available while using UnifiedStream.

Replace `192.168.1.0/24` with the actual trusted LAN subnet. Never expose these rules to an untrusted or public interface.

### UFW

```bash
sudo ufw allow from 192.168.1.0/24 to any port 47810 proto tcp comment 'UnifiedStream control'
sudo ufw allow from 192.168.1.0/24 to any port 47811 proto udp comment 'UnifiedStream media'
sudo ufw allow from 192.168.1.0/24 to 224.0.0.251 port 5353 proto udp comment 'mDNS'
sudo ufw status verbose
```

Remove the same rules with:

```bash
sudo ufw delete allow from 192.168.1.0/24 to any port 47810 proto tcp
sudo ufw delete allow from 192.168.1.0/24 to any port 47811 proto udp
sudo ufw delete allow from 192.168.1.0/24 to 224.0.0.251 port 5353 proto udp
```

### firewalld

Use the `home` zone only when the active interface is assigned to a trusted home network:

```bash
sudo firewall-cmd --zone=home --add-service=mdns --permanent
sudo firewall-cmd --zone=home --add-port=47810/tcp --permanent
sudo firewall-cmd --zone=home --add-port=47811/udp --permanent
sudo firewall-cmd --reload
sudo firewall-cmd --zone=home --list-all
```

Remove the rules with:

```bash
sudo firewall-cmd --zone=home --remove-service=mdns --permanent
sudo firewall-cmd --zone=home --remove-port=47810/tcp --permanent
sudo firewall-cmd --zone=home --remove-port=47811/udp --permanent
sudo firewall-cmd --reload
```

For raw nftables or a router firewall, create equivalent trusted-LAN rules and keep the existing default policy. Do not flush the ruleset.

## Device permissions

Inspect the virtual camera owner and the current user's groups:

```bash
ls -l /dev/video42
id
```

Most distributions assign V4L2 devices to the `video` group. Add only the intended desktop user, then sign out and back in:

```bash
sudo usermod --append --groups video "$USER"
```

Confirm the new session contains `video` with `id`. Do not use `chmod 777` or make `/dev/video*` world-writable. PipeWire audio access normally follows the logged-in user session and does not require an `audio` group.

To revoke access later:

```bash
sudo gpasswd --delete "$USER" video
```

Sign out and back in after removal.

## Remove the configuration

1. Stop all UnifiedStream streams and quit both apps.
2. Remove the desktop package or AppImage.
3. Uninstall the Android debug app.
4. Remove only the firewall rules created above.
5. Remove the dedicated module configuration files if they are no longer needed.
6. Revoke the `video` group membership if it was added solely for UnifiedStream.

If any verification step fails, continue with [Troubleshooting](troubleshooting.md).
