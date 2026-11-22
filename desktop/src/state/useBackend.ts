import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  type AudioLevel,
  type CameraStats,
  type CameraStatus,
  type ConnectionState,
  EVENTS,
  type MicStatus,
  type PairingRequest,
  type SpeakerStatus,
  type StatusSnapshot,
  type TelemetryTick,
  type TestStreamReport,
} from "../types";

/** How long an enable request may stay "starting" before the control is released again. */
const PENDING_FALLBACK_MS = 6000;

const SILENT: AudioLevel = { rms: 0, peak: 0 };

const IDLE_MIC: MicStatus = { active: false, error: null, params: null };

const IDLE_SPEAKER: SpeakerStatus = {
  active: false,
  starting: false,
  muted: false,
  // Absent until the backend says otherwise: a control is only ever added by a platform that
  // reports the capability, never assumed before the first status arrives.
  routed: null,
  error: null,
  params: null,
};

const IDLE_CAMERA: CameraStatus = {
  active: false,
  error: null,
  hint: null,
  device: null,
  params: null,
};

export interface Backend {
  status: StatusSnapshot | null;
  state: ConnectionState;
  connected: boolean;
  telemetry: TelemetryTick | null;
  pairing: PairingRequest | null;
  error: string | null;
  dismissError: () => void;

  mic: MicStatus;
  micLevel: AudioLevel;
  micPending: boolean;
  speaker: SpeakerStatus;
  speakerLevel: AudioLevel;
  camera: CameraStatus;
  cameraStats: CameraStats | null;
  cameraPending: boolean;

  testRunning: boolean;
  testReport: TestStreamReport | null;

  respondPairing: (accept: boolean) => Promise<void>;
  setCameraEnabled: (enabled: boolean) => Promise<void>;
  setMicEnabled: (enabled: boolean) => Promise<void>;
  setSpeakerEnabled: (enabled: boolean) => Promise<void>;
  setSpeakerRouting: (enabled: boolean) => Promise<void>;
  setSpeakerMuted: (muted: boolean) => Promise<void>;
  startAdvertising: () => Promise<void>;
  stopAdvertising: () => Promise<void>;
  disconnect: () => Promise<void>;
  forgetDevices: () => Promise<void>;
  startTestStream: (rateHz: number, frameBytes: number) => Promise<void>;
  stopTestStream: () => Promise<void>;
}

/**
 * Owns every piece of backend-derived state and the Tauri subscriptions that feed it.
 *
 * It lives above navigation on purpose: changing destination must never invoke a command or touch
 * a stream, so views receive this state as props and own no subscriptions of their own.
 */
export function useBackend(): Backend {
  const [status, setStatus] = useState<StatusSnapshot | null>(null);
  const [state, setState] = useState<ConnectionState>({ state: "idle" });
  const [telemetry, setTelemetry] = useState<TelemetryTick | null>(null);
  const [pairing, setPairing] = useState<PairingRequest | null>(null);
  const [testReport, setTestReport] = useState<TestStreamReport | null>(null);
  const [testRunning, setTestRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mic, setMic] = useState<MicStatus>(IDLE_MIC);
  const [micLevel, setMicLevel] = useState<AudioLevel>(SILENT);
  const [micPending, setMicPending] = useState(false);
  const [speaker, setSpeaker] = useState<SpeakerStatus>(IDLE_SPEAKER);
  const [speakerLevel, setSpeakerLevel] = useState<AudioLevel>(SILENT);
  const [camera, setCamera] = useState<CameraStatus>(IDLE_CAMERA);
  const [cameraStats, setCameraStats] = useState<CameraStats | null>(null);
  const [cameraPending, setCameraPending] = useState(false);

  // Fallback timers that release a "starting" flag when the acknowledging status event never
  // arrives. Held in refs so the event, a new request, or unmount can cancel them.
  const micTimer = useRef<number | undefined>(undefined);
  const cameraTimer = useRef<number | undefined>(undefined);

  const armPending = useCallback((timer: { current: number | undefined }, release: () => void) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(release, PENDING_FALLBACK_MS);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const snapshot = await invoke<StatusSnapshot>("get_status");
      setStatus(snapshot);
      setState(snapshot.state);
      setTestRunning(snapshot.test_stream_running);
      setMic(snapshot.mic);
      setSpeaker(snapshot.speaker);
      setCamera(snapshot.camera);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisteners = [
      listen<ConnectionState>(EVENTS.connectionState, ({ payload }) => {
        setState(payload);
        if (payload.state !== "connected") {
          // Session-scoped values must not outlive the session and read as current.
          window.clearTimeout(micTimer.current);
          window.clearTimeout(cameraTimer.current);
          setTelemetry(null);
          setTestReport(null);
          setTestRunning(false);
          setMic(IDLE_MIC);
          setMicLevel(SILENT);
          setMicPending(false);
          setSpeaker(IDLE_SPEAKER);
          setSpeakerLevel(SILENT);
          setCamera(IDLE_CAMERA);
          setCameraStats(null);
          setCameraPending(false);
        }
      }),
      listen<PairingRequest>(EVENTS.pairingRequest, ({ payload }) => setPairing(payload)),
      listen<TelemetryTick>(EVENTS.telemetry, ({ payload }) => setTelemetry(payload)),
      listen<TestStreamReport>(EVENTS.testStream, ({ payload }) => setTestReport(payload)),
      listen<MicStatus>(EVENTS.micStatus, ({ payload }) => {
        window.clearTimeout(micTimer.current);
        setMic(payload);
        setMicPending(false);
        if (!payload.active) setMicLevel(SILENT);
      }),
      listen<AudioLevel>(EVENTS.micLevel, ({ payload }) => setMicLevel(payload)),
      listen<SpeakerStatus>(EVENTS.speakerStatus, ({ payload }) => {
        setSpeaker(payload);
        if (!payload.active) setSpeakerLevel(SILENT);
      }),
      listen<AudioLevel>(EVENTS.speakerLevel, ({ payload }) => setSpeakerLevel(payload)),
      listen<CameraStatus>(EVENTS.cameraStatus, ({ payload }) => {
        window.clearTimeout(cameraTimer.current);
        setCamera(payload);
        setCameraPending(false);
        if (!payload.active) setCameraStats(null);
      }),
      listen<CameraStats>(EVENTS.cameraStats, ({ payload }) => setCameraStats(payload)),
    ];
    return () => {
      window.clearTimeout(micTimer.current);
      window.clearTimeout(cameraTimer.current);
      unlisteners.forEach((promise) => void promise.then((unlisten) => unlisten()));
    };
  }, [refresh]);

  /** Invoke a command, surface its failure, then re-read the snapshot. */
  const run = useCallback(
    async (command: string, args?: Record<string, unknown>) => {
      setError(null);
      try {
        await invoke(command, args);
        await refresh();
      } catch (cause) {
        setError(String(cause));
      }
    },
    [refresh],
  );

  const respondPairing = useCallback(
    async (accept: boolean) => {
      setPairing(null);
      await run("respond_pairing", { accept });
    },
    [run],
  );

  const setMicEnabled = useCallback(
    async (enabled: boolean) => {
      setMicPending(true);
      setMic((current) => ({ ...current, error: null }));
      try {
        await invoke("set_mic_enabled", { enabled });
        armPending(micTimer, () => setMicPending(false));
      } catch (cause) {
        setMicPending(false);
        setError(String(cause));
      }
    },
    [armPending],
  );

  const setCameraEnabled = useCallback(
    async (enabled: boolean) => {
      setCameraPending(true);
      setCamera((current) => ({ ...current, error: null, hint: null }));
      try {
        await invoke("set_camera_enabled", { enabled });
        armPending(cameraTimer, () => setCameraPending(false));
      } catch (cause) {
        setCameraPending(false);
        setError(String(cause));
      }
    },
    [armPending],
  );

  const startTestStream = useCallback(
    async (rateHz: number, frameBytes: number) => {
      await run("start_test_stream", { args: { rate_hz: rateHz, frame_bytes: frameBytes } });
      setTestRunning(true);
    },
    [run],
  );

  const stopTestStream = useCallback(async () => {
    await run("stop_test_stream");
    setTestRunning(false);
  }, [run]);

  return {
    status,
    state,
    connected: state.state === "connected",
    telemetry,
    pairing,
    error,
    dismissError: () => setError(null),
    mic,
    micLevel,
    micPending,
    speaker,
    speakerLevel,
    camera,
    cameraStats,
    cameraPending,
    testRunning,
    testReport,
    respondPairing,
    setCameraEnabled,
    setMicEnabled,
    setSpeakerEnabled: (enabled) => run("set_speaker_enabled", { enabled }),
    setSpeakerRouting: (enabled) => run("set_speaker_routing", { enabled }),
    setSpeakerMuted: (muted) => run("set_speaker_muted", { muted }),
    startAdvertising: () => run("start_advertising"),
    stopAdvertising: () => run("stop_advertising"),
    disconnect: () => run("disconnect"),
    forgetDevices: () => run("forget_devices"),
    startTestStream,
    stopTestStream,
  };
}
