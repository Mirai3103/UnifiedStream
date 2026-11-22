import { type Backend } from "../state/useBackend";
import { ConnectGuide, LinkHeader } from "../components/LinkHeader";
import { CameraStrip, MicStrip, SpeakerStrip } from "../components/Channels";
import { peerOf } from "../format";

export function Workspace({ backend }: { backend: Backend }) {
  const { state, status, telemetry, connected } = backend;
  const peer = peerOf(state);
  const needsGuide = state.state === "idle" || state.state === "discovering" || state.state === "failed";

  return (
    <div className="view">
      <header className="view-head">
        <div>
          <h1>Workspace</h1>
          <p>{peer ? `Streaming with ${peer}` : "Connect a phone to bring its camera, microphone, and speaker online."}</p>
        </div>
      </header>

      <LinkHeader state={state} status={status} telemetry={telemetry} />

      {needsGuide && (
        <ConnectGuide
          deviceName={status?.device_name ?? null}
          advertising={Boolean(status?.advertising)}
          onStartAdvertising={() => void backend.startAdvertising()}
        />
      )}

      <div className="channels">
        <CameraStrip
          camera={backend.camera}
          stats={backend.cameraStats}
          pending={backend.cameraPending}
          connected={connected}
          onToggle={() => void backend.setCameraEnabled(!backend.camera.active)}
        />
        <MicStrip
          mic={backend.mic}
          level={backend.micLevel}
          pending={backend.micPending}
          connected={connected}
          onToggle={() => void backend.setMicEnabled(!backend.mic.active)}
        />
        <SpeakerStrip
          speaker={backend.speaker}
          level={backend.speakerLevel}
          connected={connected}
          onToggle={() => void backend.setSpeakerEnabled(!(backend.speaker.active || backend.speaker.starting))}
          onRouting={() => void backend.setSpeakerRouting(!backend.speaker.routed)}
          onMute={() => void backend.setSpeakerMuted(!backend.speaker.muted)}
        />
      </div>
    </div>
  );
}
