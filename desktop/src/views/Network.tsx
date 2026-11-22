import { useState } from "react";
import { type Backend } from "../state/useBackend";
import { describeLink, peerOf } from "../format";
import { Block, Fact, StateChip } from "../components/primitives";
import { QualityChip } from "../components/LinkHeader";

/** The two-step guard for forgetting trusted devices: ask, then confirm or cancel. */
function ForgetDevices({ onConfirm }: { onConfirm: () => void }) {
  const [asking, setAsking] = useState(false);

  if (!asking) {
    return (
      <div className="btn-row">
        <button type="button" className="btn btn-danger" onClick={() => setAsking(true)}>
          Forget trusted devices
        </button>
      </div>
    );
  }
  return (
    <span className="confirm" role="group" aria-label="Confirm forgetting trusted devices">
      <span className="fine">Phones will need your approval again.</span>
      <button
        type="button"
        className="btn btn-danger"
        onClick={() => {
          setAsking(false);
          onConfirm();
        }}
      >
        Confirm forget
      </button>
      <button type="button" className="btn" onClick={() => setAsking(false)}>
        Cancel
      </button>
    </span>
  );
}

export function NetworkView({ backend }: { backend: Backend }) {
  const { status, state, telemetry, connected, testRunning, testReport } = backend;
  const [rateHz, setRateHz] = useState(60);
  const [frameBytes, setFrameBytes] = useState(4096);
  const peer = peerOf(state);

  return (
    <div className="view">
      <header className="view-head">
        <div>
          <h1>Network</h1>
          <p>Identity, local discovery, session control, and transport verification.</p>
        </div>
        <QualityChip quality={telemetry?.quality ?? "unknown"} />
      </header>

      <div className="two-up">
        <Block
          title="Local host"
          description="How phones find this device."
          trailing={<StateChip tone={status?.advertising ? "live" : "off"} label={status?.advertising ? "Advertising" : "Stopped"} />}
        >
          <dl className="facts">
            <Fact label="Name" value={status?.device_name ?? "—"} />
            <Fact label="Control port" value={status?.control_port ?? "—"} mono />
            <Fact label="Media port" value={status?.media_port ?? "—"} mono />
            <Fact label="Capabilities" value={status?.caps.join(" · ") || "—"} />
          </dl>
          <div className="btn-row">
            {status?.advertising ? (
              <button type="button" className="btn" onClick={() => void backend.stopAdvertising()}>
                Stop advertising
              </button>
            ) : (
              <button type="button" className="btn btn-primary" onClick={() => void backend.startAdvertising()}>
                Start advertising
              </button>
            )}
            <button type="button" className="btn" disabled={!connected} onClick={() => void backend.disconnect()}>
              Disconnect
            </button>
          </div>
          <ForgetDevices onConfirm={() => void backend.forgetDevices()} />
        </Block>

        <Block title="Current link" description="The active session with a phone.">
          <dl className="facts">
            <Fact label="State" value={describeLink(state)} />
            <Fact label="Peer" value={peer ?? "—"} />
            <Fact label="Session" value={state.state === "connected" ? state.session_id : "—"} mono />
            <Fact label="Download" value={telemetry ? `${telemetry.local.rx_mbps.toFixed(2)} Mbps` : "—"} mono />
          </dl>
        </Block>
      </div>

      <Block
        title="Synthetic transport test"
        description="Sends numbered frames to the phone and verifies what comes back."
        trailing={<StateChip tone={testRunning ? "live" : "off"} label={testRunning ? "Running" : "Stopped"} />}
      >
        <div className="test-controls">
          <label className="field">
            Rate
            <span className="field-input">
              <input
                type="number"
                min={1}
                max={240}
                value={rateHz}
                disabled={testRunning}
                onChange={(event) => setRateHz(Number(event.target.value))}
              />
              Hz
            </span>
          </label>
          <label className="field">
            Frame size
            <span className="field-input">
              <input
                type="number"
                min={8}
                max={65000}
                step={256}
                value={frameBytes}
                disabled={testRunning}
                onChange={(event) => setFrameBytes(Number(event.target.value))}
              />
              bytes
            </span>
          </label>
          <button
            type="button"
            className={`btn ${testRunning ? "btn-danger" : "btn-primary"}`}
            disabled={!connected}
            onClick={() => void (testRunning ? backend.stopTestStream() : backend.startTestStream(rateHz, frameBytes))}
          >
            {testRunning ? "Stop test" : "Start test"}
          </button>
        </div>
        {frameBytes > 1200 && <p className="fine">This frame size exercises fragmentation and reassembly.</p>}
        <dl className="facts">
          <Fact label="Verified" value={testReport?.verified ?? "—"} mono />
          <Fact label="Missing" value={testReport?.missing ?? "—"} mono />
          <Fact label="Corrupt" value={testReport?.corrupt ?? "—"} mono />
          <Fact label="Out of order" value={testReport?.out_of_order ?? "—"} mono />
          <Fact label="Bytes" value={testReport?.bytes ?? "—"} mono />
        </dl>
      </Block>
    </div>
  );
}
