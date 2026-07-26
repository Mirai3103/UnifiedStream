# Measured baseline — add-network-foundation

Recorded 2026-07-26 on the development hardware. This is the reference the media changes will be
measured against; the sub-50ms end-to-end target has to fit inside these numbers plus codec and
display time.

## Hardware and network

| | |
| --- | --- |
| Desktop | CachyOS, Linux 7.1.4, 192.168.1.4 |
| Phone | OnePlus CPH2767, Android 16 (SDK 36), 192.168.1.5 |
| Network | Shared 2.4/5 GHz Wi-Fi, same /24, no AP isolation |
| Build | Debug on both sides (unoptimized Rust, debuggable APK) |

## Round-trip latency

Application-level RTT is measured from the `ping`/`pong` heartbeat and smoothed (EWMA, α=0.2).

| Condition | App-reported RTT | Link quality |
| --- | --- | --- |
| Session idle (no media) | 31.9 – 43.8 ms | degraded |
| Synthetic test stream running (~1.4 Mbps) | **7.5 ms** | good |

ICMP from phone to desktop over the same link, for comparison:
`min 2.777 / avg 18.843 / max 27.889 / mdev 7.324 ms` over 12 packets, 0% loss.

**The idle figure is Wi-Fi power saving, not transport overhead.** The phone's radio parks between
packets, so a heartbeat once per second pays an association penalty that continuous traffic does
not. Latency drops by roughly 5x the moment media flows — which is the only condition that
matters. Anyone benchmarking this should measure under load, and treat an idle RTT reading as
meaningless.

## Throughput and loss

Synthetic test stream, 4096-byte frames at 60 Hz (4 fragments per frame, so the fragmentation and
reassembly path is exercised):

| Metric | Value |
| --- | --- |
| Throughput measured at the desktop | 1.40 Mbps |
| Nominal for the configuration | ~2.0 Mbps |
| Packet loss | 0.00 % |
| Jitter | 0.00 ms |
| Frame integrity failures | none |

The gap between measured and nominal is send pacing, not loss: the phone's frame interval is
`1000 / 60 = 16` under integer division, and coroutine `delay` overshoots, so the real rate is
nearer 50 fps than 60. Loss stayed at zero throughout, which is the number that matters here.

## What was exercised end to end

- mDNS discovery, phone to desktop, with the desktop advertising on IPv4 and IPv6 simultaneously —
  deduplicated to a single list entry by TXT `id`.
- Handshake, capability negotiation (`cam,mic,spk`), and session establishment.
- Fragmented media over UDP with integrity verification.
- Wi-Fi dropped mid-session and restored: session resumed with the **same** session id
  (`11931733324112871613`), confirmed in both apps' logs.
- Reconnection exhausting its five attempts, surfacing as "Could not reconnect".
- A second phone refused with `busy` while a session was live; the live session was undisturbed.
- Pairing refused for an untrusted device: the prompt went unanswered and the desktop replied
  `rejected` ("pairing was declined") and closed the connection after exactly 30.0 seconds.
- Clean `bye` teardown initiated from the phone, observed arriving at the desktop.
- Manual IP entry, including rejection of `192.168.1.999`.
- Session surviving the app being backgrounded, via the foreground service
  (`isForeground=true`, type `connectedDevice`).

## Not verified here

- **Pairing Deny pressed by hand, and a desktop-initiated disconnect.** Both need clicks in the
  Tauri window, which is a native Wayland surface with no input automation available on this
  machine. What is covered instead: the prompt-timeout rejection above exercises the same refusal
  path end to end, teardown was verified in the phone-to-desktop direction, and both button paths
  have integration tests in `crates/unifiedstream-net/tests/control_server.rs`.
- **Windows.** Out of scope for this change.
- **Release builds.** Both sides were debug builds; expect better numbers from release.
