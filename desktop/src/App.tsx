import { useState } from "react";
import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/500.css";
import "@fontsource/geist-sans/600.css";
import "@fontsource/geist-mono/400.css";
import "@fontsource/geist-mono/500.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/components.css";
import { describeFailure } from "./types";
import { useAppearance } from "./state/useAppearance";
import { useBackend } from "./state/useBackend";
import { ErrorNotice, PairingDialog, Rail, type View } from "./components/Shell";
import { Workspace } from "./views/Workspace";
import { NetworkView } from "./views/Network";
import { SettingsView } from "./views/Settings";

/**
 * The stable shell. It owns backend state and the Tauri subscriptions through `useBackend`, and the
 * appearance choice, so navigation only swaps which view renders and never touches a stream.
 */
function App() {
  const [view, setView] = useState<View>("workspace");
  const backend = useBackend();
  const { appearance, setAppearance } = useAppearance();
  const { state, error, pairing } = backend;

  const failure = state.state === "failed" ? describeFailure(state.reason) : null;

  return (
    <>
      {/* Inert while the pairing dialog is open so focus and clicks cannot reach the page behind it. */}
      <div className="app" inert={pairing !== null}>
        <Rail view={view} onNavigate={setView} status={backend.status} />
        <main className="stage" aria-label={view}>
          <div className="notices">
            {(error || failure) && <ErrorNotice message={error ?? failure ?? ""} onDismiss={error ? backend.dismissError : undefined} />}
          </div>
          {view === "workspace" && <Workspace backend={backend} />}
          {view === "network" && <NetworkView backend={backend} />}
          {view === "settings" && <SettingsView appearance={appearance} onAppearance={setAppearance} />}
        </main>
      </div>
      {pairing && <PairingDialog request={pairing} onRespond={(accept) => void backend.respondPairing(accept)} />}
    </>
  );
}

export default App;
