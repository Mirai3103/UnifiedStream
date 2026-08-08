# Troubleshooting

Start with the entry matching the visible symptom. Run elevated commands only when the entry marks them with `sudo`, and remove temporary firewall or module changes after testing.

## The phone does not discover the PC

**Diagnose**

```bash
systemctl is-active avahi-daemon
avahi-browse --resolve --terminate _unifiedstream._udp
```

**Likely causes:** the desktop is not advertising, Avahi is stopped, UDP 5353 multicast is filtered, the devices use different subnets, a VPN is active, or Wi-Fi client isolation is enabled.

**Fix:** start advertising in the desktop app, start Avahi with `sudo systemctl enable --now avahi-daemon`, allow mDNS only on the trusted LAN, and disable client isolation. As a fallback, enter the PC LAN IP and TCP port `47810` in Android's **Connect by address** form.

**Confirm:** `avahi-browse` resolves `_unifiedstream._udp`, or manual entry reaches the desktop pairing prompt.

**Undo:** remove any temporary mDNS firewall rule using the matching command in the [firewall guide](linux-installation.md#firewall).

## Discovery works but connection times out

**Diagnose**

```bash
ss -lnt | rg ':47810'
ss -lnu | rg ':47811'
```

**Likely causes:** TCP 47810 or UDP 47811 is blocked, the address is stale, or another process forced the media socket to an ephemeral fallback port.

**Fix:** quit the conflicting process so UnifiedStream can use UDP 47811, restart the desktop app, and add trusted-LAN rules for TCP 47810 and UDP 47811. Do not expose the ports publicly.

**Confirm:** Android reaches the pairing prompt, connects, and the telemetry values update.

**Undo:** remove the temporary rules with the commands in the [firewall guide](linux-installation.md#firewall).

## UnifiedStream Camera is missing

**Diagnose**

```bash
lsmod | rg '^v4l2loopback'
v4l2-ctl --list-devices
```

**Likely causes:** the module is not loaded, it was loaded without a usable output device, or the target application cached its camera list.

**Fix:** load the module as documented:

```bash
sudo modprobe v4l2loopback devices=1 card_label="UnifiedStream Camera" exclusive_caps=1
```

Then restart the target application after enabling the Camera stream.

**Confirm:** `v4l2-ctl --list-devices` lists the loopback device and the target application lists **UnifiedStream Camera**.

**Undo:** stop the stream and run `sudo modprobe -r v4l2loopback` if the module was loaded only for this test.

## UnifiedStream Camera is missing on a Windows build

**Diagnose**

```powershell
Get-ItemProperty 'HKLM:\SOFTWARE\Classes\CLSID\{6D8DD393-D871-4498-A24F-4AFFEACFC106}\InprocServer32'
Get-ItemProperty 'HKLM:\SOFTWARE\Classes\WOW6432Node\CLSID\{6D8DD393-D871-4498-A24F-4AFFEACFC106}\InprocServer32'
```

**Likely causes:** the filter is registered for one architecture only, it is not registered at all, the target application cached its camera list, or the application does not enumerate DirectShow devices.

**Fix:** register the half that is missing from an elevated prompt. The desktop's Camera card names the exact command, and it names one at a time — the 64-bit filter uses the ordinary `regsvr32`, and the 32-bit filter needs the 32-bit `regsvr32` in `SysWOW64`, which reads backwards and is the most common way this is got wrong:

```powershell
regsvr32 "C:\Program Files\UnifiedStream\UnifiedStreamCamera64.dll"
C:\Windows\SysWOW64\regsvr32.exe "C:\Program Files\UnifiedStream\UnifiedStreamCamera32.dll"
```

Then restart the target application after enabling the Camera stream.

If both are registered and the desktop no longer refuses, but one particular application still does not list the camera, check whether it is a UWP or Microsoft Store application or one that uses Media Foundation exclusively. Those do not enumerate DirectShow devices and will not show the camera; see the [coverage note in the usage guide](usage.md#on-a-windows-build).

**Confirm:** the desktop's Camera card starts without a setup hint, and the target application lists **UnifiedStream Camera**.

**Undo:** `regsvr32 /u` each DLL with the matching architecture's tool. Removal is complete: the camera disappears from every application's device list and the desktop reports it absent rather than present and broken.

## The virtual camera is busy or permission is denied

**Diagnose**

```bash
ls -l /dev/video*
id
fuser /dev/video* 2>/dev/null
```

**Likely causes:** another process holds the loopback output, the current login session lacks the `video` group, or an old desktop process did not exit.

**Fix:** close the process reported by `fuser`, quit duplicate UnifiedStream instances, or add the intended user with `sudo usermod --append --groups video "$USER"` and sign out and back in. Never use `chmod 777`.

**Confirm:** `id` lists `video`, no unintended process owns the node, and the desktop Camera card activates.

**Undo:** revoke a test-only group change with `sudo gpasswd --delete "$USER" video`, then sign out and back in.

## UnifiedStream audio nodes are missing

**Diagnose**

```bash
systemctl --user is-active pipewire pipewire-pulse wireplumber
wpctl status
pw-cli list-objects Node | sed -n '/UnifiedStream/,+8p'
```

**Likely causes:** a stream is not active, a PipeWire user service stopped, or an audio application cached its device list.

**Fix:** enable the corresponding stream on both devices. If a service is inactive, restart the user audio stack:

```bash
systemctl --user restart wireplumber pipewire pipewire-pulse
```

This interrupts current audio applications; reopen them afterward.

**Confirm:** `wpctl status` lists **UnifiedStream Microphone** or **UnifiedStream Speaker** while the relevant stream is active.

**Undo:** no persistent UnifiedStream PipeWire node is installed. Stop the stream; the runtime node disappears.

## Audio is routed but silent

**Diagnose**

```bash
wpctl status
wpctl get-volume @DEFAULT_AUDIO_SINK@
```

**Likely causes:** the wrong source or sink is selected, the stream is muted, Android media volume is zero, or routing points at a stale UnifiedStream node.

**Fix:** unmute both devices, select the intended node in the application, and disable then re-enable desktop speaker routing. If normal desktop audio remains silent after quitting UnifiedStream, select the physical output again in the system audio settings.

**Confirm:** the UnifiedStream level meter moves and sound reaches the intended endpoint.

**Undo:** restore the physical default input or output in the system audio settings.

## Android reports a denied permission

**Diagnose:** open **Android Settings → Apps → UnifiedStream → Permissions**. Camera streaming requires Camera, microphone streaming requires Microphone, and Android 13 or newer uses Notifications for the visible foreground-session notification. Network and multicast permissions are declared by the app and do not normally appear as runtime prompts.

**Fix:** grant only the permission needed for the chosen stream, return to UnifiedStream, and toggle the stream again. If **Don't ask again** was selected, permissions must be restored from system settings. Keep the foreground notification enabled while streaming in the background.

**Confirm:** Android shows the appropriate camera or microphone privacy indicator and the stream changes to active.

**Undo:** stop the stream, then revoke the permission from the same Android settings page.

## The AppImage does not start

**Diagnose**

```bash
ls -l UnifiedStream-*.AppImage
./UnifiedStream-*.AppImage 2>&1 | tee unifiedstream-startup.log
```

**Likely causes:** the executable bit is missing, FUSE 2 compatibility is unavailable, required WebKit/graphics libraries are absent, or the graphical session is unavailable.

**Fix:** run `chmod u+x UnifiedStream-*.AppImage`, install `fuse2` on CachyOS/Arch or `libfuse2t64` on Ubuntu 24.04, and retry. To distinguish a FUSE failure, run:

```bash
./UnifiedStream-*.AppImage --appimage-extract-and-run
```

Install the `.deb` on Ubuntu when the AppImage remains incompatible. Do not run the application as root.

**Confirm:** the desktop window opens and its device panel reports advertising enabled.

**Undo:** delete `unifiedstream-startup.log` if it contains no information needed for a bug report, and remove packages installed only for the test with the distribution package manager.

## Collect logs for a bug report

Run the desktop app from a terminal and keep the relevant output:

```bash
RUST_LOG=info,unifiedstream_net=debug,desktop=debug ./UnifiedStream-*.AppImage 2>&1 | tee unifiedstream.log
```

Android logs can be collected by a developer workstation:

```bash
adb logcat -d | rg 'UnifiedStream|SessionManager|DeviceDiscovery'
```

Remove IP addresses, device names, and identifiers before sharing logs publicly. Include the release tag, Linux distribution, kernel, Android version, stream involved, and exact reproduction steps.
