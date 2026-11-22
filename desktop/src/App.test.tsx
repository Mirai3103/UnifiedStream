import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  camera: { active: false, error: null, hint: null, device: null, params: null },
};

/** Render the app over a snapshot that differs from the default in the given fields. */
async function renderWith(overrides: Partial<StatusSnapshot>) {
  mocks.invoke.mockImplementation(async (command: string) =>
    command === "get_status" ? { ...snapshot, ...overrides } : undefined,
  );
  render(<App />);
  await screen.findByRole("heading", { name: "Workspace" });
}

const nav = () => within(screen.getByRole("navigation", { name: "Primary navigation" }));
const goTo = (name: string) => fireEvent.click(nav().getByRole("button", { name }));

/** Deliver a Tauri event to the subscription the app registered for it. */
const emit = (event: string, payload: unknown) => act(() => mocks.listeners.get(event)?.({ payload }));
const subscribed = (event: string) => waitFor(() => expect(mocks.listeners.has(event)).toBe(true));

beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string) => (command === "get_status" ? snapshot : undefined));
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
});

afterEach(cleanup);

describe("application shell", () => {
  it("fills the webview without rendering the presentation window mock", async () => {
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });

    expect(container.querySelector(".app")).not.toBeNull();
    expect(container.querySelector(".desktop-stage")).toBeNull();
    expect(container.querySelector(".app-window")).toBeNull();
    expect(container.querySelector(".window-chrome")).toBeNull();
    expect(container.querySelector(".traffic-lights")).toBeNull();
  });

  it("navigates without invoking a backend command and marks the current destination", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    mocks.invoke.mockClear();

    goTo("Network");
    expect(screen.getByRole("heading", { name: "Network" })).toBeInTheDocument();
    expect(nav().getByRole("button", { name: "Network" })).toHaveAttribute("aria-current", "page");
    expect(nav().getByRole("button", { name: "Workspace" })).not.toHaveAttribute("aria-current");
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("keeps live state while moving between destinations", async () => {
    render(<App />);
    await subscribed(EVENTS.telemetry);
    emit(EVENTS.telemetry, { local: { rtt_ms: 9.5, tx_mbps: 1, rx_mbps: 1, loss_pct: 0, jitter_ms: 0.5 }, peer: null, quality: "good" });
    expect(await screen.findByText("9.5")).toBeInTheDocument();

    goTo("Settings");
    goTo("Workspace");
    expect(screen.getByText("9.5")).toBeInTheDocument();
  });
});

describe("appearance", () => {
  it("defaults to the system setting and offers all three choices", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Settings");

    expect(screen.getByRole("radio", { name: "System" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Light" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Dark" })).toBeInTheDocument();
    expect(["light", "dark"]).toContain(document.documentElement.dataset.theme);
  });

  it("switches theme without losing the selected destination", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Settings");
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));

    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(screen.getByRole("heading", { name: "Appearance" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("does not disturb the connection when the theme changes", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Settings");
    mocks.invoke.mockClear();

    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("remembers an explicit choice across opens", async () => {
    const first = render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Settings");
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    first.unmount();
    delete document.documentElement.dataset.theme;

    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("falls back to the system setting and stays operable when storage throws", async () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    try {
      render(<App />);
      await screen.findByRole("heading", { name: "Workspace" });
      goTo("Settings");
      expect(screen.getByRole("radio", { name: "System" })).toBeChecked();

      fireEvent.click(screen.getByRole("radio", { name: "Light" }));
      expect(document.documentElement).toHaveAttribute("data-theme", "light");
    } finally {
      getItem.mockRestore();
      setItem.mockRestore();
    }
  });
});

describe("pairing and notices", () => {
  it("keeps pairing actionable from a non-default destination", async () => {
    render(<App />);
    await subscribed(EVENTS.pairingRequest);
    goTo("Settings");

    emit(EVENTS.pairingRequest, { device_id: "phone-123456", device_name: "Phone" });
    const dialog = await screen.findByRole("dialog", { name: "Pairing request" });
    expect(within(dialog).getByText("Phone")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("respond_pairing", { accept: true }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("moves focus into the pairing dialog and treats Escape as a refusal", async () => {
    render(<App />);
    await subscribed(EVENTS.pairingRequest);

    emit(EVENTS.pairingRequest, { device_id: "phone-123456", device_name: "Phone" });
    const dialog = await screen.findByRole("dialog", { name: "Pairing request" });
    expect(within(dialog).getByRole("button", { name: "Allow" })).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("respond_pairing", { accept: false }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps Tab inside the pairing dialog", async () => {
    render(<App />);
    await subscribed(EVENTS.pairingRequest);
    emit(EVENTS.pairingRequest, { device_id: "phone-123456", device_name: "Phone" });
    const dialog = await screen.findByRole("dialog", { name: "Pairing request" });

    const allow = within(dialog).getByRole("button", { name: "Allow" });
    const deny = within(dialog).getByRole("button", { name: "Deny" });
    // Deny precedes Allow, which is last: Tab from the last control wraps to the first.
    fireEvent.keyDown(allow, { key: "Tab" });
    expect(deny).toHaveFocus();
    fireEvent.keyDown(deny, { key: "Tab", shiftKey: true });
    expect(allow).toHaveFocus();
  });

  it("shows a connection failure from any destination without requiring a visit elsewhere", async () => {
    await renderWith({ state: { state: "failed", reason: { kind: "rejected" } } });
    goTo("Settings");

    expect(screen.getByRole("alert")).toHaveTextContent("Pairing was declined");
  });

  it("surfaces a command failure and lets the user dismiss it", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return snapshot;
      throw new Error("boom");
    });
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    fireEvent.click(screen.getByRole("switch", { name: "Phone camera" }));

    expect(await screen.findByText(/boom/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(screen.queryByText(/boom/)).toBeNull();
  });
});

describe("link header and telemetry", () => {
  it("renders live metrics and quality as text, and clears them when the connection ends", async () => {
    render(<App />);
    await subscribed(EVENTS.telemetry);
    emit(EVENTS.telemetry, {
      local: { rtt_ms: 12.5, tx_mbps: 3.2, rx_mbps: 2.1, loss_pct: 0.25, jitter_ms: 1.4 },
      peer: null,
      quality: "good",
    });

    expect(await screen.findByText("12.5")).toBeInTheDocument();
    expect(screen.getByText("3.20")).toBeInTheDocument();
    expect(screen.getByText("Link quality: Good")).toBeInTheDocument();
    expect(screen.getByText("Pixel")).toBeInTheDocument();

    emit(EVENTS.connectionState, { state: "idle" });
    await waitFor(() => expect(screen.queryByText("12.5")).not.toBeInTheDocument());
    expect(screen.queryByText("3.20")).not.toBeInTheDocument();
    expect(screen.queryByText(/Link quality/)).toBeNull();
  });

  it("states degraded and poor quality in words", async () => {
    render(<App />);
    await subscribed(EVENTS.telemetry);
    const report = { rtt_ms: 80, tx_mbps: 1, rx_mbps: 1, loss_pct: 4, jitter_ms: 9 };

    emit(EVENTS.telemetry, { local: report, peer: null, quality: "degraded" });
    expect(await screen.findByText("Link quality: Degraded")).toBeInTheDocument();
    emit(EVENTS.telemetry, { local: report, peer: null, quality: "poor" });
    expect(await screen.findByText("Link quality: Poor")).toBeInTheDocument();
  });

  it("tells the user how to connect, by name, while no phone is connected", async () => {
    await renderWith({ state: { state: "idle" } });

    expect(screen.getByRole("region", { name: "Connect a phone" })).toHaveTextContent("Studio PC");
    expect(screen.getByText("No phone", { selector: "strong" })).toBeInTheDocument();
    // Controls are unavailable, not hidden.
    expect(screen.getByRole("switch", { name: "Phone camera" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Phone microphone" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Wireless speaker" })).toBeDisabled();
  });

  it("offers to start advertising when the device is hidden", async () => {
    await renderWith({ state: { state: "discovering" }, advertising: false });

    fireEvent.click(within(screen.getByRole("region", { name: "Connect a phone" })).getByRole("button", { name: "Start advertising" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("start_advertising", undefined));
  });

  it("shows no connection guidance once a phone is connected", async () => {
    await renderWith({});
    expect(screen.queryByRole("region", { name: "Connect a phone" })).toBeNull();
  });
});

describe("media channels", () => {
  it("invokes every primary media command through accessible switches", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    fireEvent.click(screen.getByRole("switch", { name: "Phone camera" }));
    fireEvent.click(screen.getByRole("switch", { name: "Phone microphone" }));
    fireEvent.click(screen.getByRole("switch", { name: "Wireless speaker" }));

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith("set_camera_enabled", { enabled: true });
      expect(mocks.invoke).toHaveBeenCalledWith("set_mic_enabled", { enabled: true });
      expect(mocks.invoke).toHaveBeenCalledWith("set_speaker_enabled", { enabled: true });
    });
  });

  it("shows a starting state until the backend acknowledges, then releases the control", async () => {
    render(<App />);
    await subscribed(EVENTS.cameraStatus);
    fireEvent.click(screen.getByRole("switch", { name: "Phone camera" }));

    expect(await screen.findByText("Starting")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Phone camera" })).toBeDisabled();

    emit(EVENTS.cameraStatus, {
      active: true,
      error: null,
      hint: null,
      device: "UnifiedStream Camera",
      params: { codec: "mjpeg", width: 1280, height: 720, max_fps: 30 },
    });
    expect(await screen.findByText("Live")).toBeInTheDocument();
    expect(screen.getByText("1280 × 720")).toBeInTheDocument();
    expect(screen.getByText("UnifiedStream Camera")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Phone camera" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Phone camera" })).toBeChecked();
  });

  it("reports delivered camera frames and flags a stalled stream", async () => {
    await renderWith({
      camera: { ...snapshot.camera, active: true, params: { codec: "mjpeg", width: 640, height: 480, max_fps: 30 } },
    });
    await subscribed(EVENTS.cameraStats);

    emit(EVENTS.cameraStats, { frames_written: 120, decode_failures: 2, fps: 29 });
    expect(await screen.findByText("29 fps")).toBeInTheDocument();
    expect(screen.getByText("120")).toBeInTheDocument();

    emit(EVENTS.cameraStats, { frames_written: 120, decode_failures: 2, fps: 0 });
    expect(await screen.findByText("No frames arrived in the last second.")).toBeInTheDocument();
  });

  it("exposes audio levels as named meters that read zero when the stream is inactive", async () => {
    await renderWith({
      mic: { active: true, error: null, params: { codec: "opus", sample_rate: 48000, channels: 1, frame_ms: 20 } },
    });
    await subscribed(EVENTS.micLevel);
    await subscribed(EVENTS.speakerLevel);

    emit(EVENTS.micLevel, { rms: 0.42, peak: 0.8 });
    const mic = await screen.findByRole("meter", { name: "Microphone level" });
    expect(mic).toHaveAttribute("aria-valuenow", "42");
    expect(mic).toHaveAttribute("aria-valuetext", "42%");

    // The speaker stream is not running, so a stray level must not read as current.
    emit(EVENTS.speakerLevel, { rms: 0.9, peak: 0.9 });
    expect(screen.getByRole("meter", { name: "Speaker level" })).toHaveAttribute("aria-valuenow", "0");
  });

  it("clears meters and stream state when the microphone stops", async () => {
    await renderWith({
      mic: { active: true, error: null, params: { codec: "opus", sample_rate: 48000, channels: 1, frame_ms: 20 } },
    });
    await subscribed(EVENTS.micLevel);
    emit(EVENTS.micLevel, { rms: 0.5, peak: 0.6 });
    expect(await screen.findByRole("meter", { name: "Microphone level" })).toHaveAttribute("aria-valuenow", "50");

    emit(EVENTS.micStatus, { active: false, error: null, params: null });
    await waitFor(() => expect(screen.getByRole("meter", { name: "Microphone level" })).toHaveAttribute("aria-valuenow", "0"));
  });

  it("shows a muted speaker in words and toggles mute", async () => {
    await renderWith({ speaker: { ...snapshot.speaker, active: true, muted: true } });

    expect(screen.getByText("Muted")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Unmute" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("set_speaker_muted", { muted: false }));
  });

  it("offers the routing control where the platform provides the capability", async () => {
    await renderWith({ speaker: { ...snapshot.speaker, active: true, routed: false } });

    expect(screen.getByRole("switch", { name: "Route system audio" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "Route system audio" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("set_speaker_routing", { enabled: true }));
  });

  it("omits the routing control entirely where the platform reports no such capability", async () => {
    await renderWith({ speaker: { ...snapshot.speaker, active: true, routed: null } });

    // Absent, not disabled and not switched off: a control that could only fail is no control.
    expect(screen.queryByRole("switch", { name: "Route system audio" })).toBeNull();
    expect(screen.queryByText("Route system audio")).toBeNull();
    // The rest of the speaker strip is unaffected.
    expect(screen.getByRole("button", { name: "Mute" })).toBeInTheDocument();
  });

  it("marks a failed channel as needing attention and shows its reason", async () => {
    await renderWith({ mic: { active: false, error: "device busy", params: null } });

    expect(screen.getByText("Needs attention")).toBeInTheDocument();
    expect(screen.getByText("device busy")).toBeInTheDocument();
  });
});

describe("platform setup guidance", () => {
  it("renders a setup hint's message and offers its command copyable", async () => {
    const command = 'sudo modprobe v4l2loopback card_label="UnifiedStream Camera" exclusive_caps=1';
    const message = `no v4l2loopback device found — load the module with: ${command}`;
    await renderWith({
      camera: { ...snapshot.camera, error: `virtual camera unavailable: ${message}`, hint: { message, command } },
    });

    expect(screen.getByText(`virtual camera unavailable: ${message}`)).toBeInTheDocument();
    // The command is shown verbatim, exactly as the platform supplied it.
    expect(screen.getByText(command)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("copies the command and confirms it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const command = "do-the-thing --now";
    await renderWith({
      camera: { ...snapshot.camera, error: "unavailable", hint: { message: "unavailable", command } },
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(command));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("offers the registration command where the platform's camera component is not installed", async () => {
    // The platform has an implementation; what it lacks is the component that presents the camera
    // to other applications. That is a missing prerequisite with a remedy, not an unimplemented
    // platform, and the UI must show the command either way without knowing which platform it is.
    // Absolute paths and a single invocation, because the platform expands the variables before
    // handing the command over: the user pastes this into whatever elevated prompt they opened,
    // and %VAR% is literal text in PowerShell while && is a parse error in Windows PowerShell 5.1.
    const command = 'regsvr32 "C:\\Program Files\\UnifiedStream\\UnifiedStreamCamera64.dll"';
    const message = `the UnifiedStream Camera filter is not installed — register it from an elevated command prompt with: ${command} (the 32-bit filter is needed too, and the desktop names it once this one is registered)`;
    await renderWith({
      camera: { ...snapshot.camera, error: `virtual camera unavailable: ${message}`, hint: { message, command } },
    });

    expect(screen.getByText(`virtual camera unavailable: ${message}`)).toBeInTheDocument();
    expect(screen.getByText(command)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("shows a setup hint with no command as message alone", async () => {
    // Still required: a platform that genuinely has no implementation names no remedy, and the UI
    // must render guidance without a copyable command rather than an empty box.
    const message = "freebsd has no virtual camera implementation in this build";
    await renderWith({
      camera: { ...snapshot.camera, error: `virtual camera unavailable: ${message}`, hint: { message, command: null } },
    });

    expect(screen.getByText(`virtual camera unavailable: ${message}`)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("names no platform in the text it substitutes for a missing device label", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });

    expect(screen.getByText("Streams to a virtual camera device")).toBeInTheDocument();
    expect(screen.getByText("Streams to a virtual microphone device")).toBeInTheDocument();
    // Settings is excluded: the redistribution notice must name the Windows component it covers.
    for (const view of ["Workspace", "Network"]) {
      goTo(view);
      expect(document.body.textContent?.replace(/VB-CABLE/g, "")).not.toMatch(/v4l2loopback|PipeWire|WASAPI|DirectShow|Linux|Windows|Android/i);
    }
  });
});

describe("network view", () => {
  it("retains every connection action and diagnostic", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Network");

    expect(screen.getByText("47810")).toBeInTheDocument();
    expect(screen.getByText("47811")).toBeInTheDocument();
    expect(screen.getByText("18446744073709551615")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Stop advertising" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("stop_advertising", undefined));
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("disconnect", undefined));
  });

  it("asks before forgetting trusted devices, and cancelling invokes nothing", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Network");
    mocks.invoke.mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Forget trusted devices" }));
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(screen.getByRole("group", { name: "Confirm forgetting trusted devices" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Forget trusted devices" })).toBeInTheDocument();
  });

  it("forgets trusted devices only after confirmation", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Network");

    fireEvent.click(screen.getByRole("button", { name: "Forget trusted devices" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm forget" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("forget_devices", undefined));
  });

  it("disables disconnect and the transport test while no session exists", async () => {
    await renderWith({ state: { state: "idle" } });
    goTo("Network");

    expect(screen.getByRole("button", { name: "Disconnect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Start test" })).toBeDisabled();
  });

  it("runs the synthetic transport test with the chosen parameters and shows the report", async () => {
    render(<App />);
    await subscribed(EVENTS.testStream);
    goTo("Network");

    fireEvent.change(screen.getByLabelText(/Rate/), { target: { value: "30" } });
    fireEvent.change(screen.getByLabelText(/Frame size/), { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("start_test_stream", { args: { rate_hz: 30, frame_bytes: 2048 } }),
    );
    expect(await screen.findByRole("button", { name: "Stop test" })).toBeInTheDocument();
    expect(screen.getByText("This frame size exercises fragmentation and reassembly.")).toBeInTheDocument();

    emit(EVENTS.testStream, { verified: 900, corrupt: 1, missing: 2, out_of_order: 3, bytes: 123456 });
    expect(await screen.findByText("900")).toBeInTheDocument();
    expect(screen.getByText("123456")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Stop test" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("stop_test_stream", undefined));
  });
});

describe("third-party attribution", () => {
  // VB-CABLE is redistributed under a grant conditioned on the end user being able to identify the
  // component and its author, and being "in a position to donate/pay license if finding it
  // useful". That is the consideration for the licence, not a courtesy credit, so it is checked
  // rather than reviewed: deleting the notice breaks no build and fails no other test, and turns a
  // compliant product into an infringing one silently.
  //
  // Asserted against the rendered surface rather than against a source constant, because a notice
  // that exists in the source and is never rendered satisfies nothing — and a component refactor
  // is exactly how that happens.
  it("carries the redistributed component's attribution where a user can act on it", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Workspace" });
    goTo("Settings");

    const settings = screen.getByRole("heading", { name: "Third-party components" }).closest("section");
    expect(settings).not.toBeNull();
    const notice = within(settings as HTMLElement);

    // The component, so the user knows what is being attributed.
    expect(notice.getByText(/VB-CABLE/)).toBeInTheDocument();
    // Its author, so the user knows whom to pay.
    expect(notice.getByText(/VB-Audio/)).toBeInTheDocument();
    // The term the grant itself uses.
    expect((settings as HTMLElement).textContent).toMatch(/donationware/i);
    // And the means of acting on it: naming the component alone does not satisfy the grant.
    const link = notice.getByRole("link", { name: /vb-cable\.com/ });
    expect(link).toHaveAttribute("href", expect.stringContaining("vb-cable.com"));
  });

  it("shows the attribution on every platform rather than only where the component runs", async () => {
    // A platform-conditional render is the one way this notice could be dropped from a shipped
    // build without the check above noticing, so the absence of that condition is itself asserted.
    await renderWith({ caps: ["cam", "spk"] });
    goTo("Settings");

    expect(screen.getByRole("heading", { name: "Third-party components" })).toBeInTheDocument();
  });
});
