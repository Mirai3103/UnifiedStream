import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import {
  ConnectionState,
  EVENTS,
  MicLevel,
  MicStatus,
  PairingRequest,
  StatusSnapshot,
  TelemetryTick,
  TestStreamReport,
  describeFailure,
  describeState,
} from "./types";

/** Features that exist in the UI but have no implementation behind them yet. */
const PLANNED_FEATURES = [
  { id: "cam", label: "Camera", hint: "Phone camera as a webcam" },
  { id: "spk", label: "Speaker", hint: "PC audio to phone speakers" },
] as const;

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
  const [micLevel, setMicLevel] = useState<MicLevel>({ rms: 0, peak: 0 });
  // True between asking the phone for the mic and hearing its answer.
  const [micPending, setMicPending] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const snapshot = await invoke<StatusSnapshot>("get_status");
      setStatus(snapshot);
      setState(snapshot.state);
      setTestRunning(snapshot.test_stream_running);
      setMic(snapshot.mic);
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
      listen<MicLevel>(EVENTS.micLevel, (event) => setMicLevel(event.payload)),
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
          <h2>Streams</h2>
        </div>
        <div className="features">
          {PLANNED_FEATURES.map((feature) => (
            <div className="feature" key={feature.id}>
              <div>
                <div className="feature-label">{feature.label}</div>
                <div className="muted small">{feature.hint}</div>
              </div>
              <label className="toggle" title="Not available yet">
                <input type="checkbox" disabled checked={false} readOnly />
                <span className="slider" />
              </label>
            </div>
          ))}
        </div>
        <p className="muted small">
          Camera and speaker arrive in later changes.
        </p>
      </section>
    </main>
  );
}

export default App;
