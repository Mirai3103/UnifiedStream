/**
 * TypeScript mirrors of the Rust types crossing the Tauri boundary.
 *
 * Field names are snake_case because serde emits them that way; Tauri does not rename payload
 * fields, only command arguments.
 */

export type LinkQuality = "good" | "degraded" | "poor" | "unknown";

export type FailureReason =
  | { kind: "unreachable"; detail: string }
  | { kind: "version_mismatch"; detail: string }
  | { kind: "rejected" }
  | { kind: "busy" }
  | { kind: "reconnect_exhausted" }
  | { kind: "other"; detail: string };

export type ConnectionState =
  | { state: "idle" }
  | { state: "discovering" }
  | { state: "connecting"; peer_name: string }
  // session_id is a decimal string: a u64 exceeds JavaScript's safe integer range.
  | { state: "connected"; peer_name: string; session_id: string }
  | {
      state: "reconnecting";
      peer_name: string;
      attempt: number;
      max_attempts: number;
    }
  | { state: "failed"; reason: FailureReason };

export interface TelemetryReport {
  rtt_ms: number | null;
  tx_mbps: number;
  rx_mbps: number;
  loss_pct: number;
  jitter_ms: number;
}

export interface TelemetryTick {
  local: TelemetryReport;
  peer: TelemetryReport | null;
  quality: LinkQuality;
}

export interface PairingRequest {
  device_id: string;
  device_name: string;
}

export interface TestStreamReport {
  verified: number;
  corrupt: number;
  missing: number;
  out_of_order: number;
  bytes: number;
}

export interface AudioParams {
  codec: "pcm_s16le" | "opus" | "unknown";
  sample_rate: number;
  channels: number;
  frame_ms: number;
}

export interface MicStatus {
  active: boolean;
  error: string | null;
  params: AudioParams | null;
}

export interface VideoParams {
  codec: "mjpeg" | "unknown";
  width: number;
  height: number;
  max_fps: number;
}

/**
 * Setup guidance supplied by the backend's platform implementation.
 *
 * The interface renders both fields without knowing which platform produced them: `message`
 * always, `command` copyable only when the platform named one that resolves the failure.
 */
export interface SetupHint {
  message: string;
  command: string | null;
}

export interface CameraStatus {
  active: boolean;
  error: string | null;
  /** Platform-supplied guidance for the last failure, when the platform supplied any. */
  hint: SetupHint | null;
  /** Opaque platform label for the device being written to. Never parsed here. */
  device: string | null;
  params: VideoParams | null;
}

/** Delivered-frame counters, 1 Hz while the camera stream is active. */
export interface CameraStats {
  frames_written: number;
  decode_failures: number;
  /** Frames written over the last second; 0 means a stalled (not stopped) stream. */
  fps: number;
}

export interface SpeakerStatus {
  active: boolean;
  starting: boolean;
  muted: boolean;
  /**
   * Whether the system output is routed to the virtual sink, or `null` where the platform has
   * no routing concept. `null` is absence, not "off": the control is omitted entirely.
   */
  routed: boolean | null;
  error: string | null;
  params: AudioParams | null;
}

/** One live-level sample for a meter — microphone or speaker. Both fields 0..1. */
export interface AudioLevel {
  rms: number;
  peak: number;
}

export interface StatusSnapshot {
  device_name: string;
  device_id: string;
  advertising: boolean;
  state: ConnectionState;
  control_port: number;
  media_port: number;
  caps: string[];
  test_stream_running: boolean;
  mic: MicStatus;
  speaker: SpeakerStatus;
  camera: CameraStatus;
}

/** Event names emitted by the Rust side. Mirrors `app::events`. */
export const EVENTS = {
  connectionState: "connection-state",
  pairingRequest: "pairing-request",
  telemetry: "telemetry",
  testStream: "test-stream",
  micStatus: "mic-status",
  micLevel: "mic-level",
  speakerStatus: "speaker-status",
  speakerLevel: "speaker-level",
  cameraStatus: "camera-status",
  cameraStats: "camera-stats",
} as const;

/** Human-readable text for a failure. */
export function describeFailure(reason: FailureReason): string {
  switch (reason.kind) {
    case "unreachable":
      return `Could not reach device: ${reason.detail}`;
    case "version_mismatch":
      return `Incompatible version: ${reason.detail}`;
    case "rejected":
      return "Pairing was declined";
    case "busy":
      return "Device is already paired with another phone";
    case "reconnect_exhausted":
      return "Could not reconnect";
    case "other":
      return reason.detail;
  }
}

/** Short label for the connection state chip. */
export function describeState(state: ConnectionState): string {
  switch (state.state) {
    case "idle":
      return "Idle";
    case "discovering":
      return "Waiting for a phone";
    case "connecting":
      return `Connecting to ${state.peer_name}`;
    case "connected":
      return state.peer_name;
    case "reconnecting":
      return `Reconnecting (${state.attempt}/${state.max_attempts})`;
    case "failed":
      return describeFailure(state.reason);
  }
}
