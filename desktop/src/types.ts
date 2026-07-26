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

export interface StatusSnapshot {
  device_name: string;
  device_id: string;
  advertising: boolean;
  state: ConnectionState;
  control_port: number;
  media_port: number;
  caps: string[];
  test_stream_running: boolean;
}

/** Event names emitted by the Rust side. Mirrors `app::events`. */
export const EVENTS = {
  connectionState: "connection-state",
  pairingRequest: "pairing-request",
  telemetry: "telemetry",
  testStream: "test-stream",
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
