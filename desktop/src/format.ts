import { type ConnectionState, type LinkQuality, describeFailure } from "./types";

/** Short label for the link between the two endpoints. `types.describeState` names the peer instead. */
export function describeLink(state: ConnectionState): string {
  switch (state.state) {
    case "idle":
      return "Not connected";
    case "discovering":
      return "Waiting for a phone";
    case "connecting":
      return "Connecting…";
    case "connected":
      return "Connected";
    case "reconnecting":
      return `Reconnecting (${state.attempt}/${state.max_attempts})`;
    case "failed":
      return describeFailure(state.reason);
  }
}

export const QUALITY_LABEL: Record<LinkQuality, string> = {
  good: "Good",
  degraded: "Degraded",
  poor: "Poor",
  unknown: "No data",
};

/** Peer name for the states that have one. */
export function peerOf(state: ConnectionState): string | null {
  return state.state === "connected" || state.state === "connecting" || state.state === "reconnecting"
    ? state.peer_name
    : null;
}

export const percent = (level: number) => Math.round(Math.min(1, Math.max(0, level)) * 100);
