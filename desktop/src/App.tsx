import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LayoutDashboard, Moon, Network, Settings, Sun, X, type LucideIcon } from "lucide-react";
import "@fontsource/space-grotesk/400.css";
import "@fontsource/space-grotesk/500.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/600.css";
import appIconUrl from "../src-tauri/icons/icon.svg";
import "./App.css";
import {
  type AudioLevel,
  type CameraStats,
  type CameraStatus,
  type ConnectionState,
  EVENTS,
  type LinkQuality,
  type MicStatus,
  type PairingRequest,
  type SpeakerStatus,
  type StatusSnapshot,
  type TelemetryTick,
  type TestStreamReport,
  describeFailure,
  describeState,
} from "./types";

type View = "workspace" | "network" | "settings";
type Theme = "dark" | "light";

const NAV_ITEMS: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "workspace", label: "Workspace", icon: LayoutDashboard },
  { view: "network", label: "Network", icon: Network },
  { view: "settings", label: "Settings", icon: Settings },
];

const IDLE_SPEAKER: SpeakerStatus = {
  active: false,
  starting: false,
  muted: false,
  // Absent until the backend says otherwise: a control is only ever added by a platform that
  // reports the capability, never assumed before the first status arrives.
  routed: null,
  error: null,
  params: null,
};

const IDLE_CAMERA: CameraStatus = {
  active: false,
  error: null,
  hint: null,
  device: null,
  params: null,
};

function StatusLight({ active, label }: { active: boolean; label: string }) {
  return (
    <span className="status-label">
      <span className={`status-light ${active ? "is-live" : ""}`} aria-hidden="true" />
      {label}
    </span>
  );
}

function Panel({
  title,
  eyebrow,
  trailing,
  className = "",
  children,
}: {
  title: string;
  eyebrow?: string;
  trailing?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={`glass-panel ${className}`}>
      <header className="panel-heading">
        <div>
          {eyebrow && <span className="eyebrow">{eyebrow}</span>}
          <h2>{title}</h2>
        </div>
        {trailing}
      </header>
      {children}
    </section>
  );
}

function SwitchControl({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: () => void;
}) {
  return (
    <label className="switch-control">
      <span className="sr-only">{label}</span>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
      <span className="switch-track" aria-hidden="true" />
    </label>
  );
}

function LevelMeter({ level, label, violet = false }: { level: AudioLevel; label: string; violet?: boolean }) {
  const rms = Math.round(level.rms * 100);
  const peak = Math.round(level.peak * 100);
  return (
    <div className={`level-meter ${violet ? "is-violet" : ""}`} aria-label={`${label}: ${rms}%`}>
      <span className="level-fill" style={{ width: `${rms}%` }} />
      <span className="level-peak" style={{ left: `${peak}%` }} />
    </div>
  );
}

function MetricCard({ label, value, unit, accent = "teal" }: { label: string; value: string; unit?: string; accent?: "teal" | "violet" | "amber" }) {
  return (
    <div className={`metric-card metric-${accent}`}>
      <span className="eyebrow">{label}</span>
      <div className="metric-value">
        {value} {unit && <small>{unit}</small>}
      </div>
    </div>
  );
}

function App() {
  const [view, setView] = useState<View>("workspace");
  const [theme, setTheme] = useState<Theme>("dark");
  const [status, setStatus] = useState<StatusSnapshot | null>(null);
  const [state, setState] = useState<ConnectionState>({ state: "idle" });
  const [telemetry, setTelemetry] = useState<TelemetryTick | null>(null);
  const [pairing, setPairing] = useState<PairingRequest | null>(null);
  const [testReport, setTestReport] = useState<TestStreamReport | null>(null);
  const [testRunning, setTestRunning] = useState(false);
  const [frameBytes, setFrameBytes] = useState(4096);
  const [rateHz, setRateHz] = useState(60);
  const [error, setError] = useState<string | null>(null);
  const [mic, setMic] = useState<MicStatus>({ active: false, error: null, params: null });
  const [micLevel, setMicLevel] = useState<AudioLevel>({ rms: 0, peak: 0 });
  const [micPending, setMicPending] = useState(false);
  const [speaker, setSpeaker] = useState<SpeakerStatus>(IDLE_SPEAKER);
  const [speakerLevel, setSpeakerLevel] = useState<AudioLevel>({ rms: 0, peak: 0 });
  const [camera, setCamera] = useState<CameraStatus>(IDLE_CAMERA);
  const [cameraStats, setCameraStats] = useState<CameraStats | null>(null);
  const [cameraPending, setCameraPending] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const snapshot = await invoke<StatusSnapshot>("get_status");
      setStatus(snapshot);
      setState(snapshot.state);
      setTestRunning(snapshot.test_stream_running);
      setMic(snapshot.mic);
      setSpeaker(snapshot.speaker);
      setCamera(snapshot.camera);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    void refresh();
    const unlisteners = [
      listen<ConnectionState>(EVENTS.connectionState, ({ payload }) => {
        setState(payload);
        if (payload.state !== "connected") {
          setTelemetry(null);
          setTestReport(null);
          setTestRunning(false);
          setMic({ active: false, error: null, params: null });
          setMicLevel({ rms: 0, peak: 0 });
          setMicPending(false);
          setSpeaker(IDLE_SPEAKER);
          setSpeakerLevel({ rms: 0, peak: 0 });
          setCamera(IDLE_CAMERA);
          setCameraStats(null);
          setCameraPending(false);
        }
      }),
      listen<PairingRequest>(EVENTS.pairingRequest, ({ payload }) => setPairing(payload)),
      listen<TelemetryTick>(EVENTS.telemetry, ({ payload }) => setTelemetry(payload)),
      listen<TestStreamReport>(EVENTS.testStream, ({ payload }) => setTestReport(payload)),
      listen<MicStatus>(EVENTS.micStatus, ({ payload }) => {
        setMic(payload);
        setMicPending(false);
        if (!payload.active) setMicLevel({ rms: 0, peak: 0 });
      }),
      listen<AudioLevel>(EVENTS.micLevel, ({ payload }) => setMicLevel(payload)),
      listen<SpeakerStatus>(EVENTS.speakerStatus, ({ payload }) => {
        setSpeaker(payload);
        if (!payload.active) setSpeakerLevel({ rms: 0, peak: 0 });
      }),
      listen<AudioLevel>(EVENTS.speakerLevel, ({ payload }) => setSpeakerLevel(payload)),
      listen<CameraStatus>(EVENTS.cameraStatus, ({ payload }) => {
        setCamera(payload);
        setCameraPending(false);
        if (!payload.active) setCameraStats(null);
      }),
      listen<CameraStats>(EVENTS.cameraStats, ({ payload }) => setCameraStats(payload)),
    ];
    return () => unlisteners.forEach((promise) => void promise.then((unlisten) => unlisten()));
  }, [refresh]);

  const run = useCallback(async (command: string, args?: Record<string, unknown>) => {
    setError(null);
    try {
      await invoke(command, args);
      await refresh();
    } catch (cause) {
      setError(String(cause));
    }
  }, [refresh]);

  const respondPairing = async (accept: boolean) => {
    setPairing(null);
    await run("respond_pairing", { accept });
  };

  const toggleTestStream = async () => {
    if (testRunning) {
      await run("stop_test_stream");
      setTestRunning(false);
    } else {
      await run("start_test_stream", { args: { rate_hz: rateHz, frame_bytes: frameBytes } });
      setTestRunning(true);
    }
  };

  const toggleMic = async () => {
    setMicPending(true);
    setMic((current) => ({ ...current, error: null }));
    try {
      await invoke("set_mic_enabled", { enabled: !mic.active });
      window.setTimeout(() => setMicPending(false), 6000);
    } catch (cause) {
      setMicPending(false);
      setError(String(cause));
    }
  };

  const toggleCamera = async () => {
    setCameraPending(true);
    setCamera((current) => ({ ...current, error: null, hint: null }));
    try {
      await invoke("set_camera_enabled", { enabled: !camera.active });
      window.setTimeout(() => setCameraPending(false), 6000);
    } catch (cause) {
      setCameraPending(false);
      setError(String(cause));
    }
  };

  const connected = state.state === "connected";
  const quality: LinkQuality = telemetry?.quality ?? "unknown";
  const peerName = state.state === "connected" || state.state === "connecting" || state.state === "reconnecting" ? state.peer_name : null;

  return (
    <main className="app-layout">
          <aside className="sidebar">
            <div className="brand-mark" aria-label="UnifiedStream home"><img src={appIconUrl} alt="" /></div>
            <nav aria-label="Primary navigation">
              {NAV_ITEMS.map(({ view: item, label, icon: NavIcon }) => (
                <button key={item} className={`nav-item ${view === item ? "is-selected" : ""}`} aria-current={view === item ? "page" : undefined} onClick={() => setView(item)}>
                  <NavIcon className="nav-icon" size={18} strokeWidth={1.8} aria-hidden="true" />
                  {label}
                </button>
              ))}
            </nav>
            <div className="sidebar-status">
              <span className="eyebrow">This device</span>
              <strong>{status?.device_name ?? "Starting…"}</strong>
              <span className="mono muted">{status ? `${status.device_id.slice(0, 12)}…` : "—"}</span>
              <StatusLight active={Boolean(status?.advertising)} label={status?.advertising ? "Discoverable" : "Hidden"} />
            </div>
          </aside>

          <div className="content-column">
            <div className="compact-nav" role="navigation" aria-label="Compact navigation">
              {NAV_ITEMS.map(({ view: item, label, icon: NavIcon }) => (
                <button key={item} aria-pressed={view === item} onClick={() => setView(item)}><NavIcon size={17} strokeWidth={1.8} aria-hidden="true" />{label}</button>
              ))}
            </div>

            {(error || state.state === "failed") && (
              <div className="global-notice is-error" role="alert">
                <strong>Action required</strong>
                <span>{error ?? (state.state === "failed" ? describeFailure(state.reason) : "")}</span>
                {error && <button className="icon-button" aria-label="Dismiss error" onClick={() => setError(null)}><X size={18} aria-hidden="true" /></button>}
              </div>
            )}
            {pairing && (
              <div className="global-notice" role="dialog" aria-modal="true" aria-label="Pairing request">
                <div><strong>{pairing.device_name}</strong><span> wants to connect · </span><span className="mono muted">{pairing.device_id.slice(0, 8)}…</span></div>
                <div className="button-row"><button className="primary" onClick={() => void respondPairing(true)}>Allow</button><button onClick={() => void respondPairing(false)}>Deny</button></div>
              </div>
            )}

            {view === "workspace" && (
              <div className="view-stack">
                <div className="title-row">
                  <div><span className="eyebrow">Workspace</span><h1>Media bridge</h1><p>{peerName ? `Streaming console for ${peerName}` : "Connect a phone to bring your sources online."}</p></div>
                  <span className={`state-pill state-${state.state}`}><span className="status-light is-live" aria-hidden="true" />{describeState(state)}</span>
                </div>
                <section className="telemetry-grid" aria-label="Live network telemetry">
                  <MetricCard label="Latency" value={telemetry?.local.rtt_ms != null ? telemetry.local.rtt_ms.toFixed(1) : "—"} unit="ms" />
                  <MetricCard label="Bandwidth" value={(telemetry?.local.tx_mbps ?? 0).toFixed(2)} unit="Mbps up" accent="violet" />
                  <MetricCard label="Packet loss" value={(telemetry?.local.loss_pct ?? 0).toFixed(2)} unit="%" accent={quality === "poor" ? "amber" : "teal"} />
                  <MetricCard label="Jitter" value={(telemetry?.local.jitter_ms ?? 0).toFixed(2)} unit="ms" />
                </section>
                <div className="media-grid">
                  <Panel title="Camera" eyebrow="Video source" className="camera-panel" trailing={<StatusLight active={camera.active} label={camera.active ? "Live" : cameraPending ? "Starting" : "Off"} />}>
                    <div className="camera-content">
                      <div className="camera-preview" aria-label="Camera stream preview status">
                        <span className={`preview-orb ${camera.active ? "is-active" : ""}`} />
                        <strong>{camera.active ? "CAMERA LIVE" : "PREVIEW STANDBY"}</strong>
                        <span>{camera.params ? `${camera.params.width} × ${camera.params.height} · ${camera.params.codec.toUpperCase()}` : "MJPEG virtual camera"}</span>
                      </div>
                      <div className="control-stack">
                        <div className="control-row"><div><strong>Phone camera</strong><span>{camera.device ?? "Streams to a virtual camera device"}</span></div><SwitchControl label="Phone camera" checked={camera.active} disabled={!connected || cameraPending} onChange={() => void toggleCamera()} /></div>
                        <dl className="detail-grid"><div><dt>Delivered</dt><dd>{camera.active ? `${cameraStats?.fps ?? 0} fps` : "—"}</dd></div><div><dt>Frames</dt><dd>{cameraStats?.frames_written ?? "—"}</dd></div><div><dt>Decode failures</dt><dd>{cameraStats?.decode_failures ?? "—"}</dd></div></dl>
                        {camera.error && <p className="error-copy" role="alert">{camera.error}</p>}
                        {/* The message always reaches the user through the error line above, which the
                            backend renders from this same hint. The box adds the copyable command, so a
                            platform that supplies none produces no box rather than an empty one. */}
                        {camera.hint?.command && <div className="hint-box"><code>{camera.hint.command}</code><button onClick={() => void navigator.clipboard.writeText(camera.hint?.command ?? "")}>Copy</button></div>}
                      </div>
                    </div>
                  </Panel>

                  <Panel title="Microphone" eyebrow="Audio input" trailing={<StatusLight active={mic.active} label={mic.active ? "Live" : micPending ? "Starting" : "Off"} />}>
                    <div className="control-row"><div><strong>Phone microphone</strong><span>{mic.active ? `${mic.params?.codec ?? "audio"} · ${(mic.params?.sample_rate ?? 48000) / 1000} kHz` : "Streams to a virtual microphone device"}</span></div><SwitchControl label="Phone microphone" checked={mic.active} disabled={!connected || micPending} onChange={() => void toggleMic()} /></div>
                    <LevelMeter level={mic.active ? micLevel : { rms: 0, peak: 0 }} label="Microphone level" />
                    {mic.error && <p className="error-copy" role="alert">{mic.error}</p>}
                  </Panel>

                  <Panel title="Speaker" eyebrow="Audio output" trailing={<StatusLight active={speaker.active} label={speaker.active ? "Live" : speaker.starting ? "Starting" : "Off"} />}>
                    <div className="control-row"><div><strong>Wireless speaker</strong><span>{speaker.active ? `${speaker.params?.codec ?? "audio"} · ${speaker.params?.channels === 2 ? "stereo" : "mono"}` : "Send PC audio to the phone"}</span></div><SwitchControl label="Wireless speaker" checked={speaker.active || speaker.starting} disabled={!connected || speaker.starting} onChange={() => void run("set_speaker_enabled", { enabled: !(speaker.active || speaker.starting) })} /></div>
                    <LevelMeter level={speaker.active ? speakerLevel : { rms: 0, peak: 0 }} label="Speaker level" violet />
                    {speaker.active && <>{speaker.routed !== null && <div className="control-row compact"><div><strong>Route system audio</strong><span>Restore previous output when stopped</span></div><SwitchControl label="Route system audio" checked={speaker.routed} onChange={() => void run("set_speaker_routing", { enabled: !speaker.routed })} /></div>}<button onClick={() => void run("set_speaker_muted", { muted: !speaker.muted })}>{speaker.muted ? "Unmute" : "Mute"}</button></>}
                    {speaker.error && <p className="error-copy" role="alert">{speaker.error}</p>}
                  </Panel>
                </div>
              </div>
            )}

            {view === "network" && (
              <div className="view-stack">
                <div className="title-row"><div><span className="eyebrow">Network</span><h1>Connection & diagnostics</h1><p>Identity, local discovery, session control, and transport verification.</p></div><span className={`quality-badge quality-${quality}`}>{quality}</span></div>
                <div className="two-column-grid">
                  <Panel title="Local host" eyebrow="Discovery" trailing={<StatusLight active={Boolean(status?.advertising)} label={status?.advertising ? "Advertising" : "Stopped"} />}>
                    <dl className="detail-grid"><div><dt>Name</dt><dd>{status?.device_name ?? "—"}</dd></div><div><dt>Control</dt><dd>{status?.control_port ?? "—"}</dd></div><div><dt>Media</dt><dd>{status?.media_port ?? "—"}</dd></div><div><dt>Capabilities</dt><dd>{status?.caps.join(" · ") || "—"}</dd></div></dl>
                    <div className="button-row">{status?.advertising ? <button onClick={() => void run("stop_advertising")}>Stop advertising</button> : <button className="primary" onClick={() => void run("start_advertising")}>Start advertising</button>}<button onClick={() => void run("disconnect")} disabled={!connected}>Disconnect</button><button onClick={() => void run("forget_devices")}>Forget trusted devices</button></div>
                  </Panel>
                  <Panel title="Current link" eyebrow="Session">
                    <dl className="detail-grid"><div><dt>State</dt><dd>{describeState(state)}</dd></div><div><dt>Peer</dt><dd>{peerName ?? "—"}</dd></div><div><dt>Session</dt><dd className="mono">{state.state === "connected" ? state.session_id : "—"}</dd></div><div><dt>Download</dt><dd>{(telemetry?.local.rx_mbps ?? 0).toFixed(2)} Mbps</dd></div></dl>
                  </Panel>
                </div>
                <Panel title="Synthetic transport test" eyebrow="Diagnostics" trailing={<StatusLight active={testRunning} label={testRunning ? "Running" : "Stopped"} />}>
                  <div className="diagnostic-controls"><label>Rate <input type="number" min={1} max={240} value={rateHz} disabled={testRunning} onChange={(event) => setRateHz(Number(event.target.value))} /><span>Hz</span></label><label>Frame <input type="number" min={8} max={65000} step={256} value={frameBytes} disabled={testRunning} onChange={(event) => setFrameBytes(Number(event.target.value))} /><span>bytes</span></label><button className={testRunning ? "danger" : "primary"} disabled={!connected} onClick={() => void toggleTestStream()}>{testRunning ? "Stop test" : "Start test"}</button></div>
                  {frameBytes > 1200 && <p className="muted">This frame size exercises fragmentation and reassembly.</p>}
                  <dl className="detail-grid diagnostic-results"><div><dt>Verified</dt><dd>{testReport?.verified ?? "—"}</dd></div><div><dt>Missing</dt><dd>{testReport?.missing ?? "—"}</dd></div><div><dt>Corrupt</dt><dd>{testReport?.corrupt ?? "—"}</dd></div><div><dt>Out of order</dt><dd>{testReport?.out_of_order ?? "—"}</dd></div><div><dt>Bytes</dt><dd>{testReport?.bytes ?? "—"}</dd></div></dl>
                </Panel>
              </div>
            )}

            {view === "settings" && (
              <div className="view-stack settings-view">
                <div className="title-row"><div><span className="eyebrow">Settings</span><h1>Appearance</h1><p>Presentation preferences stay local and never affect an active stream.</p></div></div>
                <Panel title="Theme" eyebrow="Interface">
                  <div className="theme-options" role="radiogroup" aria-label="Theme"><button role="radio" aria-checked={theme === "dark"} className={theme === "dark" ? "is-selected" : ""} onClick={() => setTheme("dark")}><span className="theme-swatch dark-swatch"><Moon size={16} aria-hidden="true" /></span>Dark</button><button role="radio" aria-checked={theme === "light"} className={theme === "light" ? "is-selected" : ""} onClick={() => setTheme("light")}><span className="theme-swatch light-swatch"><Sun size={16} aria-hidden="true" /></span>Light</button></div>
                </Panel>
                <Panel title="Runtime contract" eyebrow="About"><p className="body-copy">The redesign uses the existing discovery, session, media, and telemetry backend. Illustrative design controls without runtime support are intentionally not interactive.</p><dl className="detail-grid"><div><dt>Version</dt><dd className="mono">0.1.0</dd></div><div><dt>Desktop backend</dt><dd>Platform virtual devices</dd></div><div><dt>Protocol</dt><dd>Trusted LAN</dd></div></dl></Panel>
              </div>
            )}
          </div>
    </main>
  );
}

export default App;
