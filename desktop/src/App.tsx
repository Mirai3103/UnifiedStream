import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import {
  AudioLevel,
  CameraStats,
  CameraStatus,
  ConnectionState,
  EVENTS,
  MicStatus,
  PairingRequest,
  SpeakerStatus,
  StatusSnapshot,
  TelemetryTick,
  TestStreamReport,
  describeFailure,
  describeState,
} from "./types";

const IDLE_SPEAKER: SpeakerStatus = {
  active: false,
  starting: false,
  muted: false,
  routed: false,
  error: null,
  params: null,
};

const IDLE_CAMERA: CameraStatus = {
  active: false,
  error: null,
  hint: null,
  params: null,
  device: null,
};

function App() {
  const [status, setStatus] = useState<StatusSnapshot | null>(null);
  const [state, setState] = useState<ConnectionState>({ state: "idle" });
  const [telemetry, setTelemetry] = useState<TelemetryTick | null>(null);
  const [pairing, setPairing] = useState<PairingRequest | null>(null);
  const [testReport, setTestReport] = useState<TestStreamReport | null>(null);
  const [testRunning, setTestRunning] = useState(false);
  const [frameBytes, setFrameBytes] = useState(4096);
  const [rateHz, setRateHz] = useState(60);
  const [error, setError] = useState<string | null>(null);
  const [mic, setMic] = useState<MicStatus>({
    active: false,
    error: null,
    params: null,
  });
  const [micLevel, setMicLevel] = useState<AudioLevel>({ rms: 0, peak: 0 });
  // True between asking the phone for the mic and hearing its answer.
  const [micPending, setMicPending] = useState(false);
  const [speaker, setSpeaker] = useState<SpeakerStatus>(IDLE_SPEAKER);
  const [speakerLevel, setSpeakerLevel] = useState<AudioLevel>({
    rms: 0,
    peak: 0,
  });
  const [camera, setCamera] = useState<CameraStatus>(IDLE_CAMERA);
  const [cameraStats, setCameraStats] = useState<CameraStats | null>(null);
  // True between asking the phone for the camera and hearing its answer.
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
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();

    const unlisteners = [
      listen<ConnectionState>(EVENTS.connectionState, (event) => {
        setState(event.payload);
        // A dead session must not leave stale numbers on the dashboard.
        if (event.payload.state !== "connected") {
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
      listen<PairingRequest>(EVENTS.pairingRequest, (event) =>
        setPairing(event.payload),
      ),
      listen<TelemetryTick>(EVENTS.telemetry, (event) =>
        setTelemetry(event.payload),
      ),
      listen<TestStreamReport>(EVENTS.testStream, (event) =>
        setTestReport(event.payload),
      ),
      listen<MicStatus>(EVENTS.micStatus, (event) => {
        setMic(event.payload);
        setMicPending(false);
        if (!event.payload.active) setMicLevel({ rms: 0, peak: 0 });
      }),
      listen<AudioLevel>(EVENTS.micLevel, (event) => setMicLevel(event.payload)),
      listen<SpeakerStatus>(EVENTS.speakerStatus, (event) => {
        setSpeaker(event.payload);
        if (!event.payload.active) setSpeakerLevel({ rms: 0, peak: 0 });
      }),
      listen<AudioLevel>(EVENTS.speakerLevel, (event) =>
        setSpeakerLevel(event.payload),
      ),
      listen<CameraStatus>(EVENTS.cameraStatus, (event) => {
        setCamera(event.payload);
        setCameraPending(false);
        if (!event.payload.active) setCameraStats(null);
      }),
      listen<CameraStats>(EVENTS.cameraStats, (event) =>
        setCameraStats(event.payload),
      ),
    ];

    return () => {
      unlisteners.forEach((p) => void p.then((un) => un()));
    };
  }, [refresh]);

  const run = useCallback(
    async (command: string, args?: Record<string, unknown>) => {
      setError(null);
      try {
        await invoke(command, args);
        await refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [refresh],
  );

  const respondPairing = async (accept: boolean) => {
    setPairing(null);
    await run("respond_pairing", { accept });
  };

  const toggleTestStream = async () => {
    if (testRunning) {
      await run("stop_test_stream");
      setTestRunning(false);
      return;
    }
    await run("start_test_stream", {
      args: { rate_hz: rateHz, frame_bytes: frameBytes },
    });
    setTestRunning(true);
  };

  const toggleMic = async () => {
    setMicPending(true);
    setMic((current) => ({ ...current, error: null }));
    try {
      await invoke("set_mic_enabled", { enabled: !mic.active });
      // The answer arrives via the mic-status event; if the phone never replies,
      // stop showing a spinner after its ack timeout would have fired.
      window.setTimeout(() => setMicPending(false), 6000);
    } catch (e) {
      setMicPending(false);
      setError(String(e));
    }
  };

  const toggleCamera = async () => {
    setCameraPending(true);
    setCamera((current) => ({ ...current, error: null, hint: null }));
    try {
      await invoke("set_camera_enabled", { enabled: !camera.active });
      // The answer arrives via the camera-status event; if the phone never replies,
      // stop showing a spinner after its ack timeout would have fired.
      window.setTimeout(() => setCameraPending(false), 6000);
    } catch (e) {
      setCameraPending(false);
      setError(String(e));
    }
  };

  const toggleSpeaker = async () => {
    await run("set_speaker_enabled", {
      enabled: !(speaker.active || speaker.starting),
    });
  };

  const toggleSpeakerMute = async () => {
    await run("set_speaker_muted", { muted: !speaker.muted });
  };

  const toggleSpeakerRouting = async () => {
    await run("set_speaker_routing", { enabled: !speaker.routed });
  };

  const connected = state.state === "connected";
  const quality = telemetry?.quality ?? "unknown";

  return (
    <main className="app">
      <header className="header">
        <div>
          <h1>UnifiedStream</h1>
          <p className="subtitle">
            {status ? status.device_name : "Starting up…"}
          </p>
        </div>
        <span className={`chip chip-${state.state}`}>
          {describeState(state)}
        </span>
      </header>

      {error && (
        <div className="glass banner banner-error" role="alert">
          {error}
        </div>
      )}

      {pairing && (
        <div className="glass banner banner-pairing" role="dialog">
          <div>
            <strong>{pairing.device_name}</strong> wants to connect.
            <span className="muted"> {pairing.device_id.slice(0, 8)}…</span>
          </div>
          <div className="row">
            <button className="primary" onClick={() => respondPairing(true)}>
              Allow
            </button>
            <button onClick={() => respondPairing(false)}>Deny</button>
          </div>
        </div>
      )}

      <section className="glass panel">
        <div className="panel-head">
          <h2>This device</h2>
          <span className={`dot dot-${status?.advertising ? "on" : "off"}`} />
        </div>

        <dl className="grid">
          <div>
            <dt>Control port</dt>
            <dd>{status?.control_port ?? "—"}</dd>
          </div>
          <div>
            <dt>Media port</dt>
            <dd>{status?.media_port ?? "—"}</dd>
          </div>
          <div>
            <dt>Device ID</dt>
            <dd className="mono small">
              {status ? `${status.device_id.slice(0, 13)}…` : "—"}
            </dd>
          </div>
        </dl>

        <div className="row">
          {status?.advertising ? (
            <button onClick={() => run("stop_advertising")}>
              Stop advertising
            </button>
          ) : (
            <button className="primary" onClick={() => run("start_advertising")}>
              Start advertising
            </button>
          )}
          <button onClick={() => run("disconnect")} disabled={!connected}>
            Disconnect
          </button>
          <button onClick={() => run("forget_devices")}>Forget devices</button>
        </div>
      </section>

      <section className="glass panel">
        <div className="panel-head">
          <h2>Link quality</h2>
          <span className={`quality quality-${quality}`}>{quality}</span>
        </div>

        {connected ? (
          <dl className="grid metrics">
            <div>
              <dt>Ping</dt>
              <dd>
                {telemetry?.local.rtt_ms != null
                  ? `${telemetry.local.rtt_ms.toFixed(1)} ms`
                  : "—"}
              </dd>
            </div>
            <div>
              <dt>Sent</dt>
              <dd>{`${(telemetry?.local.tx_mbps ?? 0).toFixed(2)} Mbps`}</dd>
            </div>
            <div>
              <dt>Received</dt>
              <dd>{`${(telemetry?.local.rx_mbps ?? 0).toFixed(2)} Mbps`}</dd>
            </div>
            <div>
              <dt>Packet loss</dt>
              <dd>{`${(telemetry?.local.loss_pct ?? 0).toFixed(2)} %`}</dd>
            </div>
            <div>
              <dt>Jitter</dt>
              <dd>{`${(telemetry?.local.jitter_ms ?? 0).toFixed(2)} ms`}</dd>
            </div>
          </dl>
        ) : (
          <p className="muted">
            {state.state === "failed"
              ? describeFailure(state.reason)
              : "Connect a phone to see live metrics."}
          </p>
        )}
      </section>

      <section className="glass panel">
        <div className="panel-head">
          <h2>Test stream</h2>
          <span className="muted small">
            No codecs yet — this proves the transport.
          </span>
        </div>

        <div className="row controls">
          <label>
            Rate
            <input
              type="number"
              min={1}
              max={240}
              value={rateHz}
              disabled={testRunning}
              onChange={(e) => setRateHz(Number(e.target.value))}
            />
            <span className="muted small">Hz</span>
          </label>
          <label>
            Frame
            <input
              type="number"
              min={8}
              max={65000}
              step={256}
              value={frameBytes}
              disabled={testRunning}
              onChange={(e) => setFrameBytes(Number(e.target.value))}
            />
            <span className="muted small">bytes</span>
          </label>
          <button
            className={testRunning ? "" : "primary"}
            onClick={toggleTestStream}
            disabled={!connected}
          >
            {testRunning ? "Stop" : "Start"}
          </button>
        </div>

        {frameBytes > 1200 && (
          <p className="muted small">
            Above 1200 bytes, so this exercises fragmentation and reassembly.
          </p>
        )}

        {testReport && (
          <dl className="grid metrics">
            <div>
              <dt>Verified</dt>
              <dd>{testReport.verified}</dd>
            </div>
            <div>
              <dt>Missing</dt>
              <dd>{testReport.missing}</dd>
            </div>
            <div>
              <dt>Corrupt</dt>
              <dd>{testReport.corrupt}</dd>
            </div>
            <div>
              <dt>Out of order</dt>
              <dd>{testReport.out_of_order}</dd>
            </div>
          </dl>
        )}
      </section>

      <section className="glass panel">
        <div className="panel-head">
          <h2>Microphone</h2>
          <span className={`dot dot-${mic.active ? "on" : "off"}`} />
        </div>

        <div className="features">
          <div className="feature">
            <div>
              <div className="feature-label">Phone microphone</div>
              <div className="muted small">
                {mic.active
                  ? `Live — "UnifiedStream Microphone" is available as an input device (${
                      mic.params ? `${mic.params.codec}, ${mic.params.sample_rate / 1000} kHz` : "…"
                    })`
                  : micPending
                    ? "Waiting for the phone…"
                    : "Ask the phone to stream its mic to this PC"}
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={mic.active}
                disabled={!connected || micPending}
                onChange={() => void toggleMic()}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {mic.active && (
          <div className="level-meter" aria-label="microphone level">
            <div
              className="level-fill"
              style={{ width: `${Math.round(micLevel.rms * 100)}%` }}
            />
            <div
              className="level-peak"
              style={{ left: `${Math.round(micLevel.peak * 100)}%` }}
            />
          </div>
        )}

        {mic.error && (
          <p className="muted small error-text" role="alert">
            {mic.error}
          </p>
        )}
      </section>

      <section className="glass panel">
        <div className="panel-head">
          <h2>Speaker</h2>
          <span className={`dot dot-${speaker.active ? "on" : "off"}`} />
        </div>

        <div className="features">
          <div className="feature">
            <div>
              <div className="feature-label">Wireless speaker</div>
              <div className="muted small">
                {speaker.active
                  ? `Live — play audio into "UnifiedStream Speaker" to hear it on the phone (${
                      speaker.params
                        ? `${speaker.params.codec}, ${speaker.params.sample_rate / 1000} kHz, ${
                            speaker.params.channels === 2 ? "stereo" : "mono"
                          }`
                        : "…"
                    })`
                  : speaker.starting
                    ? "Waiting for the phone…"
                    : "Send this PC's audio to the phone's speaker"}
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={speaker.active || speaker.starting}
                disabled={!connected || speaker.starting}
                onChange={() => void toggleSpeaker()}
              />
              <span className="slider" />
            </label>
          </div>

          {speaker.active && (
            <div className="feature">
              <div>
                <div className="feature-label">Route system audio</div>
                <div className="muted small">
                  {speaker.routed
                    ? "All system audio goes to the phone; your previous output is restored on stop"
                    : "Make the virtual sink the default output, so everything plays on the phone"}
                </div>
              </div>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={speaker.routed}
                  onChange={() => void toggleSpeakerRouting()}
                />
                <span className="slider" />
              </label>
            </div>
          )}
        </div>

        {speaker.active && (
          <>
            <div className="level-meter" aria-label="speaker level">
              <div
                className="level-fill"
                style={{ width: `${Math.round(speakerLevel.rms * 100)}%` }}
              />
              <div
                className="level-peak"
                style={{ left: `${Math.round(speakerLevel.peak * 100)}%` }}
              />
            </div>
            <div className="row">
              <button onClick={() => void toggleSpeakerMute()}>
                {speaker.muted ? "Unmute" : "Mute"}
              </button>
              {speaker.muted && (
                <span className="muted small">
                  Muted — the phone hears silence
                </span>
              )}
            </div>
          </>
        )}

        {speaker.error && (
          <p className="muted small error-text" role="alert">
            {speaker.error}
          </p>
        )}
      </section>

      <section className="glass panel">
        <div className="panel-head">
          <h2>Camera</h2>
          <span className={`dot dot-${camera.active ? "on" : "off"}`} />
        </div>

        <div className="features">
          <div className="feature">
            <div>
              <div className="feature-label">Phone camera</div>
              <div className="muted small">
                {camera.active
                  ? `Live — "UnifiedStream Camera" is available as a webcam${
                      camera.device ? ` on ${camera.device}` : ""
                    }${
                      camera.params
                        ? ` (${camera.params.width}x${camera.params.height}, up to ${camera.params.max_fps} fps)`
                        : ""
                    }`
                  : cameraPending
                    ? "Waiting for the phone…"
                    : "Ask the phone to stream its camera as this PC's webcam"}
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={camera.active}
                disabled={!connected || cameraPending}
                onChange={() => void toggleCamera()}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {camera.active && (
          <dl className="grid metrics">
            <div>
              <dt>Delivered</dt>
              {/* 0 fps with the stream active means stalled, not stopped. */}
              <dd>{`${cameraStats?.fps ?? 0} fps`}</dd>
            </div>
            <div>
              <dt>Frames</dt>
              <dd>{cameraStats?.frames_written ?? 0}</dd>
            </div>
            <div>
              <dt>Undecodable</dt>
              <dd>{cameraStats?.decode_failures ?? 0}</dd>
            </div>
          </dl>
        )}

        {camera.error && (
          <p className="muted small error-text" role="alert">
            {camera.error}
          </p>
        )}

        {camera.hint && (
          <div className="row">
            <code className="mono small">{camera.hint}</code>
            <button
              onClick={() => void navigator.clipboard.writeText(camera.hint ?? "")}
            >
              Copy
            </button>
          </div>
        )}
      </section>
    </main>
  );
}

export default App;
