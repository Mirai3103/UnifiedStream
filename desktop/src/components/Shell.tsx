import { useEffect, useRef, type KeyboardEvent } from "react";
import { LayoutDashboard, Network, Settings, TriangleAlert, X, type LucideIcon } from "lucide-react";
import appIconUrl from "../../src-tauri/icons/icon.svg";
import { type PairingRequest, type StatusSnapshot } from "../types";
import { StateChip } from "./primitives";

export type View = "workspace" | "network" | "settings";

const NAV_ITEMS: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "workspace", label: "Workspace", icon: LayoutDashboard },
  { view: "network", label: "Network", icon: Network },
  { view: "settings", label: "Settings", icon: Settings },
];

/** Navigation plus this device's identity. Wide: sidebar. Narrow: icon rail, then top tabs. */
export function Rail({
  view,
  onNavigate,
  status,
}: {
  view: View;
  onNavigate: (view: View) => void;
  status: StatusSnapshot | null;
}) {
  return (
    <aside className="rail">
      <div className="brand">
        <img src={appIconUrl} alt="" />
        <span className="brand-name">UnifiedStream</span>
      </div>
      <nav className="nav" aria-label="Primary navigation">
        {NAV_ITEMS.map(({ view: item, label, icon: Icon }) => (
          <button
            key={item}
            type="button"
            className="nav-item"
            aria-current={view === item ? "page" : undefined}
            onClick={() => onNavigate(item)}
          >
            <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
            <span className="nav-label">{label}</span>
          </button>
        ))}
      </nav>
      <section className="device-card" aria-label="This device">
        <span className="caption">This device</span>
        <strong>{status?.device_name ?? "Starting…"}</strong>
        <span className="caption mono">{status ? `${status.device_id.slice(0, 12)}…` : "—"}</span>
        <StateChip
          tone={status?.advertising ? "live" : "off"}
          label={status?.advertising ? "Discoverable" : "Hidden"}
        />
      </section>
    </aside>
  );
}

/** A failure the user must see from any destination. */
export function ErrorNotice({ message, onDismiss }: { message: string; onDismiss?: () => void }) {
  return (
    <div className="notice" role="alert">
      <TriangleAlert size={20} aria-hidden="true" />
      <div className="notice-text">
        <strong>Action required</strong>
        <span>{message}</span>
      </div>
      {onDismiss && (
        <button type="button" className="btn btn-icon" aria-label="Dismiss error" onClick={onDismiss}>
          <X size={18} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}

/**
 * A phone asking to connect. Modal, because the session cannot proceed without an answer:
 * focus moves in, Tab stays inside, Escape declines, and focus returns to where it was.
 */
export function PairingDialog({
  request,
  onRespond,
}: {
  request: PairingRequest;
  onRespond: (accept: boolean) => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const allow = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    allow.current?.focus();
    return () => previous?.focus?.();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onRespond(false);
      return;
    }
    if (event.key !== "Tab") return;
    const buttons = dialog.current?.querySelectorAll<HTMLButtonElement>("button");
    if (!buttons?.length) return;
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div className="scrim">
      <div
        ref={dialog}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="pairing-title"
        onKeyDown={onKeyDown}
      >
        <h2 id="pairing-title">Pairing request</h2>
        <p>
          <strong>{request.device_name}</strong> wants to connect to this device.
        </p>
        <p className="caption mono">{request.device_id.slice(0, 8)}…</p>
        <div className="actions">
          <button type="button" className="btn" onClick={() => onRespond(false)}>
            Deny
          </button>
          <button ref={allow} type="button" className="btn btn-primary" onClick={() => onRespond(true)}>
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}
