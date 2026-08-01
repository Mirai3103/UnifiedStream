# Linux MVP smoke-test record

This record captures the local verification performed for prerelease `v0.1.0-mvp.1` on 2026-08-01. It is also the source for the implementation pull request's smoke-test section.

## Environments

| Component | Environment |
| --- | --- |
| Linux desktop | CachyOS, kernel `7.1.5-1-cachyos`, Wayland |
| Audio | PipeWire `1.6.8` with WirePlumber |
| Virtual camera | In-tree `v4l2loopback` `0.15.4` |
| Android | OnePlus CPH2767, Android 16 (API 36), physical USB device |
| Network | Trusted `192.168.1.0/24` Wi-Fi LAN |

Ubuntu runtime testing was intentionally skipped for this MVP. The `.deb` artifact was built successfully and its package name, version, architecture, metadata, filename, and SHA-256 checksum were inspected locally.

## Artifact installation and startup

The complete artifact set was built with:

```bash
scripts/build-release-artifacts.sh v0.1.0-mvp.1 /tmp/unifiedstream-release-test
sha256sum --check /tmp/unifiedstream-release-test/SHA256SUMS
```

The CachyOS AppImage passed both native FUSE startup and the documented fallback:

```bash
chmod u+x UnifiedStream-0.1.0-mvp.1-linux-x86_64.AppImage
./UnifiedStream-0.1.0-mvp.1-linux-x86_64.AppImage
./UnifiedStream-0.1.0-mvp.1-linux-x86_64.AppImage --appimage-extract-and-run
```

Both runs opened the Wayland desktop window, listened on TCP `47810` and UDP `47811`, and advertised `_unifiedstream._udp.local.`. The debug APK installed and cold-launched successfully:

```bash
adb install -r UnifiedStream-0.1.0-mvp.1-android-universal-debug.apk
adb shell am start -W -n com.laffy.unifiedstream/.MainActivity
```

## End-to-end results

| Check | Result | Evidence |
| --- | --- | --- |
| mDNS discovery | Pass | Android found `cachyos-x8664` at `192.168.1.4:47810` with `cam`, `mic`, and `spk` capabilities. |
| Control session | Pass | A session was established with the physical phone and telemetry updated. |
| Camera | Pass | Android reported `Streaming 1280x720`; V4L2 exposed `UnifiedStream Camera` at 30 FPS, and FFmpeg captured a real 1280×720 frame from `/dev/video0`. |
| Microphone | Pass | Android reported `Live`; PipeWire exposed `UnifiedStream Microphone`, and a 2.997-second capture measured `-22.8 dB` mean and `-6.5 dB` peak. |
| Speaker | Pass | PipeWire exposed `UnifiedStream Speaker`; `pw-play` routed the local ALSA front-center sample and Android reported `Playing PC audio`. |
| Android permissions | Pass | Camera, microphone, and notification runtime permissions were granted and the corresponding active states appeared. |

## Recovery results

- Discovery/session: stopping the desktop changed Android to `Reconnecting`; restarting the AppImage restored the same session and all three requested streams.
- Video: disabling and re-enabling Camera recreated the active stream, after which FFmpeg captured another valid 1280×720 frame.
- Audio: disabling Speaker removed its PipeWire sink; re-enabling it recreated `UnifiedStream Speaker` and restored the Android `Playing PC audio` state.

## Known limitations

- Ubuntu and native `.deb` installation were not available for this local smoke test by explicit project decision.
- The speaker path was confirmed through PipeWire routing and Android playback state; the test did not record the phone's physical speaker acoustically.
- The Wayland compositor logged non-fatal GBM buffer warnings while the Tauri window remained functional.
