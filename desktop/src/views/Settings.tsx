import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { type Appearance } from "../state/useAppearance";
import { Block, Fact } from "../components/primitives";

const OPTIONS: { value: Appearance; label: string; icon: LucideIcon }[] = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
];

export function SettingsView({
  appearance,
  onAppearance,
}: {
  appearance: Appearance;
  onAppearance: (value: Appearance) => void;
}) {
  return (
    <div className="view">
      <header className="view-head">
        <div>
          <h1>Settings</h1>
          <p>Preferences stay on this device and never affect an active stream.</p>
        </div>
      </header>

      <Block title="Appearance" description="System follows your operating system and changes with it.">
        <div className="options" role="radiogroup" aria-label="Appearance">
          {OPTIONS.map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              type="button"
              role="radio"
              className="option"
              aria-checked={appearance === value}
              onClick={() => onAppearance(value)}
            >
              <Icon size={18} aria-hidden="true" />
              {label}
            </button>
          ))}
        </div>
      </Block>

      <Block title="About">
        <dl className="facts">
          <Fact label="Version" value="0.1.0" mono />
          <Fact label="Devices" value="Platform virtual devices" />
          <Fact label="Network" value="Trusted local network" />
        </dl>
      </Block>

      {/* Not conditional on the platform. VB-CABLE ships only in the Windows build, but a notice
          rendered only where the component runs is a notice a refactor can remove from every
          other build without failing anything — and the obligation is on what is distributed,
          which is checked here rather than at run time. */}
      <Block title="Third-party components">
        <p className="body-copy">
          The Windows microphone plays the phone’s audio into <strong>VB-CABLE Virtual Audio Device</strong>, donationware by{" "}
          <strong>VB-Audio Software</strong>, which is redistributed with this application under the terms its author grants. If you
          find it useful, it can be donated for or licensed at{" "}
          <a href="https://vb-cable.com" target="_blank" rel="noreferrer">
            vb-cable.com
          </a>
          .
        </p>
      </Block>
    </div>
  );
}
