import { CircleDashed, Monitor, SignalHigh, SignalLow, SignalMedium, Smartphone } from "lucide-react";
import { type ConnectionState, type LinkQuality, type StatusSnapshot, type TelemetryTick } from "../types";
import { QUALITY_LABEL, describeLink, peerOf } from "../format";
import { StateChip, type Tone } from "./primitives";

const QUALITY_VIEW = {
  good: { icon: SignalHigh, tone: "live" },
  degraded: { icon: SignalMedium, tone: "warn" },
  poor: { icon: SignalLow, tone: "danger" },
  unknown: { icon: CircleDashed, tone: "off" },
} as const satisfies Record<LinkQuality, { icon: typeof SignalHigh; tone: Tone }>;

/** Link quality as text plus a signal-strength glyph, so it never relies on color. */
export function QualityChip({ quality }: { quality: LinkQuality }) {
  const { icon, tone } = QUALITY_VIEW[quality];
  return <StateChip tone={tone} icon={icon} label={`Link quality: ${QUALITY_LABEL[quality]}`} />;
}

function Figure({ label, value, unit }: { label: string; value: string; unit: string }) {
  return (
    <div className="figure">
      <dt>{label}</dt>
      <dd>
        {value}
        {value !== "—" && <small>{unit}</small>}
      </dd>
    </div>
  );
}

const fixed = (value: number | null | undefined, digits: number) => (value == null ? "—" : value.toFixed(digits));

/**
 * The first thing the workspace answers: is a phone connected, and how healthy is the link.
 * This PC, the link between, and the phone read left to right; the figures describe the link.
 */
export function LinkHeader({
  state,
  status,
  telemetry,
}: {
  state: ConnectionState;
  status: StatusSnapshot | null;
  telemetry: TelemetryTick | null;
}) {
  const peer = peerOf(state);
  const local = telemetry?.local;
  const connected = state.state === "connected";

  return (
    <section className="link" aria-label="Connection">
      <div className="link-path">
        <div className="endpoint">
          <span className="endpoint-icon">
            <Monitor size={22} aria-hidden="true" />
          </span>
          <div className="endpoint-text">
            <span className="caption">This PC</span>
            <strong>{status?.device_name ?? "Starting…"}</strong>
          </div>
        </div>

        <div className="wire" data-state={state.state}>
          <span className="wire-label">{describeLink(state)}</span>
          <span className="wire-line" aria-hidden="true" />
          {connected && <QualityChip quality={telemetry?.quality ?? "unknown"} />}
        </div>

        <div className="endpoint">
          <div className="endpoint-text">
            <span className="caption">Phone</span>
            <strong>{peer ?? "No phone"}</strong>
          </div>
          <span className="endpoint-icon">
            <Smartphone size={22} aria-hidden="true" />
          </span>
        </div>
      </div>

      <dl className="figures" aria-label="Live network telemetry">
        <Figure label="Latency" value={fixed(local?.rtt_ms, 1)} unit="ms" />
        <Figure label="Upload" value={fixed(local?.tx_mbps, 2)} unit="Mbps" />
        <Figure label="Packet loss" value={fixed(local?.loss_pct, 2)} unit="%" />
        <Figure label="Jitter" value={fixed(local?.jitter_ms, 2)} unit="ms" />
      </dl>
    </section>
  );
}

/** Shown instead of a row of dashes while no phone is connected. */
export function ConnectGuide({
  deviceName,
  advertising,
  onStartAdvertising,
}: {
  deviceName: string | null;
  advertising: boolean;
  onStartAdvertising: () => void;
}) {
  return (
    <section className="guide" aria-label="Connect a phone">
      <div>
        <h2>No phone connected</h2>
        <p>
          {advertising
            ? `Open UnifiedStream on your phone and choose “${deviceName ?? "this device"}”. You’ll be asked to approve it here the first time.`
            : "This device is hidden, so phones can’t find it. Make it discoverable to connect."}
        </p>
      </div>
      {!advertising && (
        <button type="button" className="btn btn-primary" onClick={onStartAdvertising}>
          Start advertising
        </button>
      )}
    </section>
  );
}
