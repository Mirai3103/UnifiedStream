import { type ReactNode } from "react";
import { Camera, Mic, Volume2, VolumeX, type LucideIcon } from "lucide-react";
import {
  type AudioLevel,
  type AudioParams,
  type CameraStats,
  type CameraStatus,
  type MicStatus,
  type SpeakerStatus,
} from "../types";
import { CopyButton, ErrorLine, Meter, StateChip, Switch, type Tone } from "./primitives";

const SILENT: AudioLevel = { rms: 0, peak: 0 };

const CODEC_LABEL: Record<AudioParams["codec"], string> = {
  pcm_s16le: "PCM 16-bit",
  opus: "Opus",
  unknown: "Audio",
};

const codecOf = (params: AudioParams | null) => CODEC_LABEL[params?.codec ?? "unknown"];

const micSummary = (params: AudioParams | null) => `${codecOf(params)} · ${(params?.sample_rate ?? 48000) / 1000} kHz`;

const speakerSummary = (params: AudioParams | null) =>
  `${codecOf(params)}${params ? ` · ${params.channels === 2 ? "stereo" : "mono"}` : ""}`;

/** The frame every channel shares: identity, state, the enable control, then its own body. */
function Strip({
  className = "",
  icon: Icon,
  title,
  subtitle,
  chip,
  control,
  children,
}: {
  className?: string;
  icon: LucideIcon;
  title: string;
  subtitle: string;
  chip: ReactNode;
  control: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className={`strip ${className}`} aria-label={title}>
      <header className="strip-head">
        <span className="strip-icon">
          <Icon size={20} aria-hidden="true" />
        </span>
        <div className="strip-title">
          <div className="strip-title-row">
            <h2>{title}</h2>
            {chip}
          </div>
          <p>{subtitle}</p>
        </div>
        {control}
      </header>
      <div className="strip-body">{children}</div>
    </section>
  );
}

function chipTone(active: boolean, pending: boolean, error: string | null): { tone: Tone; label: string } {
  if (active) return { tone: "live", label: "Live" };
  if (pending) return { tone: "busy", label: "Starting" };
  if (error) return { tone: "danger", label: "Needs attention" };
  return { tone: "off", label: "Off" };
}

export function CameraStrip({
  camera,
  stats,
  pending,
  connected,
  onToggle,
}: {
  camera: CameraStatus;
  stats: CameraStats | null;
  pending: boolean;
  connected: boolean;
  onToggle: () => void;
}) {
  const { tone, label } = chipTone(camera.active, pending, camera.error);
  const stalled = camera.active && stats !== null && stats.fps === 0;
  const params = camera.params;

  return (
    <Strip
      className="strip-camera"
      icon={Camera}
      title="Camera"
      subtitle={camera.device ?? "Streams to a virtual camera device"}
      chip={<StateChip tone={tone} label={label} />}
      control={<Switch label="Phone camera" checked={camera.active} disabled={!connected || pending} onChange={onToggle} />}
    >
      <div className={`viewfinder ${camera.active ? "is-live" : ""}`} role="img" aria-label="Camera stream status">
        {camera.active ? (
          <>
            <span className="viewfinder-size">{params ? `${params.width} × ${params.height}` : "Receiving"}</span>
            <span>{params ? `${params.codec.toUpperCase()} · up to ${params.max_fps} fps` : "Waiting for format"}</span>
          </>
        ) : (
          <span>{pending ? "Starting camera…" : "Camera is off"}</span>
        )}
      </div>

      <dl className="readout">
        <div>
          <dt>Delivered</dt>
          <dd>{camera.active ? `${stats?.fps ?? 0} fps` : "—"}</dd>
        </div>
        <div>
          <dt>Frames</dt>
          <dd>{stats?.frames_written ?? "—"}</dd>
        </div>
        <div>
          <dt>Decode failures</dt>
          <dd>{stats?.decode_failures ?? "—"}</dd>
        </div>
      </dl>

      {stalled && <p className="fine">No frames arrived in the last second.</p>}
      {camera.error && <ErrorLine>{camera.error}</ErrorLine>}
      {/* The message always reaches the user through the error line above, which the backend
          renders from this same hint. The box adds the copyable command, so a platform that
          supplies none produces no box rather than an empty one. */}
      {camera.hint?.command && (
        <div className="hint">
          <code>{camera.hint.command}</code>
          <CopyButton text={camera.hint.command} />
        </div>
      )}
    </Strip>
  );
}

export function MicStrip({
  mic,
  level,
  pending,
  connected,
  onToggle,
}: {
  mic: MicStatus;
  level: AudioLevel;
  pending: boolean;
  connected: boolean;
  onToggle: () => void;
}) {
  const { tone, label } = chipTone(mic.active, pending, mic.error);
  return (
    <Strip
      icon={Mic}
      title="Microphone"
      subtitle={mic.active ? micSummary(mic.params) : "Streams to a virtual microphone device"}
      chip={<StateChip tone={tone} label={label} />}
      control={<Switch label="Phone microphone" checked={mic.active} disabled={!connected || pending} onChange={onToggle} />}
    >
      <Meter level={mic.active ? level : SILENT} label="Microphone level" />
      {mic.error && <ErrorLine>{mic.error}</ErrorLine>}
    </Strip>
  );
}

export function SpeakerStrip({
  speaker,
  level,
  connected,
  onToggle,
  onRouting,
  onMute,
}: {
  speaker: SpeakerStatus;
  level: AudioLevel;
  connected: boolean;
  onToggle: () => void;
  onRouting: () => void;
  onMute: () => void;
}) {
  const muted = speaker.active && speaker.muted;
  const chip = muted ? (
    <StateChip tone="warn" icon={VolumeX} label="Muted" />
  ) : (
    <StateChip {...chipTone(speaker.active, speaker.starting, speaker.error)} />
  );

  return (
    <Strip
      icon={Volume2}
      title="Speaker"
      subtitle={speaker.active ? speakerSummary(speaker.params) : "Send PC audio to the phone"}
      chip={chip}
      control={
        <Switch
          label="Wireless speaker"
          checked={speaker.active || speaker.starting}
          disabled={!connected || speaker.starting}
          onChange={onToggle}
        />
      }
    >
      <Meter level={speaker.active ? level : SILENT} label="Speaker level" />
      {speaker.active && (
        <>
          {/* Absent where the platform reports no routing concept: a control that could only
              fail is no control, and `null` is absence rather than "off". */}
          {speaker.routed !== null && (
            <div className="row">
              <div>
                <strong>Route system audio</strong>
                <span>Restore previous output when stopped</span>
              </div>
              <Switch label="Route system audio" checked={speaker.routed} onChange={onRouting} />
            </div>
          )}
          <div className="btn-row">
            <button type="button" className="btn" onClick={onMute}>
              {speaker.muted ? "Unmute" : "Mute"}
            </button>
          </div>
        </>
      )}
      {speaker.error && <ErrorLine>{speaker.error}</ErrorLine>}
    </Strip>
  );
}
