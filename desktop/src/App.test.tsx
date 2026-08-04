import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { EVENTS, type StatusSnapshot } from "./types";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(event, handler);
    return () => mocks.listeners.delete(event);
  }),
}));

const snapshot: StatusSnapshot = {
  device_name: "Studio PC",
  device_id: "12345678-1234-1234-1234-123456789abc",
  advertising: true,
  state: { state: "connected", peer_name: "Pixel", session_id: "18446744073709551615" },
  control_port: 47810,
  media_port: 47811,
  caps: ["cam", "mic", "spk"],
  test_stream_running: false,
  mic: { active: false, error: null, params: null },
  speaker: { active: false, starting: false, muted: false, routed: false, error: null, params: null },
  camera: { active: false, error: null, hint: null, params: null, device: null },
};

beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string) => command === "get_status" ? snapshot : undefined);
});

afterEach(cleanup);

describe("desktop application shell", () => {
  it("fills the webview without rendering the presentation window mock", async () => {
    const { container } = render(<App />);
    await screen.findByText("Media bridge");

    expect(container.firstElementChild).toHaveClass("app-layout");
    expect(container.querySelector(".desktop-stage")).toBeNull();
    expect(container.querySelector(".app-window")).toBeNull();
    expect(container.querySelector(".window-chrome")).toBeNull();
    expect(container.querySelector(".traffic-lights")).toBeNull();
  });

  it("navigates without invoking a backend command", async () => {
    render(<App />);
    await screen.findByText("Media bridge");
    mocks.invoke.mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Network" }));
    expect(screen.getByText("Connection & diagnostics")).toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("switches theme without losing the selected destination", async () => {
    render(<App />);
    await screen.findByText("Media bridge");
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("radio", { name: /Light/ }));

    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(screen.getByText("Appearance")).toBeInTheDocument();
  });

  it("keeps pairing actionable from a non-default destination", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.listeners.has(EVENTS.pairingRequest)).toBe(true));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    mocks.listeners.get(EVENTS.pairingRequest)?.({ payload: { device_id: "phone-123456", device_name: "Phone" } });
    expect(await screen.findByRole("dialog", { name: "Pairing request" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("respond_pairing", { accept: true }));
  });

  it("renders live metrics and clears them when the connection ends", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.listeners.has(EVENTS.telemetry)).toBe(true));
    mocks.listeners.get(EVENTS.telemetry)?.({ payload: { local: { rtt_ms: 12.5, tx_mbps: 3.2, rx_mbps: 2.1, loss_pct: 0.25, jitter_ms: 1.4 }, peer: null, quality: "good" } });
    expect(await screen.findByText("12.5")).toBeInTheDocument();

    mocks.listeners.get(EVENTS.connectionState)?.({ payload: { state: "idle" } });
    await waitFor(() => expect(screen.queryByText("12.5")).not.toBeInTheDocument());
  });

  it("invokes every primary media command through accessible controls", async () => {
    render(<App />);
    await screen.findByText("Media bridge");
    fireEvent.click(screen.getByRole("checkbox", { name: "Phone camera" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Phone microphone" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Wireless speaker" }));

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith("set_camera_enabled", { enabled: true });
      expect(mocks.invoke).toHaveBeenCalledWith("set_mic_enabled", { enabled: true });
      expect(mocks.invoke).toHaveBeenCalledWith("set_speaker_enabled", { enabled: true });
    });
  });
});
