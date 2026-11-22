import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Circle, Copy, LoaderCircle, TriangleAlert, type LucideIcon } from "lucide-react";
import { type AudioLevel } from "../types";
import { percent } from "../format";

/** An on/off control. A native checkbox, so keyboard, focus, and form semantics come for free. */
export function Switch({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: () => void;
}) {
  return (
    <label className="switch">
      <span className="sr-only">{label}</span>
      <input type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={onChange} />
    </label>
  );
}

export type Tone = "live" | "busy" | "warn" | "danger" | "off";

/**
 * A state stated in words and a distinct shape, never in color alone: a pulsing dot for live, a
 * spinner for in-progress, a triangle for a fault, a hollow ring for off.
 */
export function StateChip({ tone, label, icon }: { tone: Tone; label: string; icon?: LucideIcon }) {
  const Icon = icon;
  return (
    <span className={`chip ${tone === "live" ? "chip-live" : tone === "warn" ? "chip-warn" : tone === "danger" ? "chip-danger" : ""}`}>
      {Icon ? (
        <Icon size={14} aria-hidden="true" />
      ) : tone === "live" ? (
        <span className="dot" aria-hidden="true" />
      ) : tone === "busy" ? (
        <LoaderCircle size={14} className="spin" aria-hidden="true" />
      ) : tone === "danger" || tone === "warn" ? (
        <TriangleAlert size={14} aria-hidden="true" />
      ) : (
        <Circle size={14} aria-hidden="true" />
      )}
      {label}
    </span>
  );
}

/** A live audio level with a peak marker, exposed to assistive technology as a meter. */
export function Meter({ level, label }: { level: AudioLevel; label: string }) {
  const rms = percent(level.rms);
  const peak = percent(level.peak);
  return (
    <div className="meter-row">
      <div
        className="meter"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={rms}
        aria-valuetext={`${rms}%`}
      >
        <span className="meter-fill" style={{ transform: `scaleX(${rms / 100})` }} />
        <span className="meter-peak" style={{ left: `calc(${peak}% - 1px)` }} />
      </div>
      <span className="meter-value" aria-hidden="true">
        {rms}%
      </span>
    </div>
  );
}

export function ErrorLine({ children }: { children: ReactNode }) {
  return (
    <p className="error-line" role="alert">
      <TriangleAlert size={16} aria-hidden="true" />
      <span>{children}</span>
    </p>
  );
}

/** Copy a command to the clipboard and say so; clipboard access can be refused. */
export function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  };

  return (
    <button type="button" className="btn btn-quiet" onClick={() => void copy()}>
      {copied ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

/** A titled group of related content on the Network and Settings views. */
export function Block({
  title,
  description,
  trailing,
  children,
}: {
  title: string;
  description?: string;
  trailing?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="block">
      <header className="block-head">
        <div>
          <h2>{title}</h2>
          {description && <p>{description}</p>}
        </div>
        {trailing}
      </header>
      {children}
    </section>
  );
}

/** One labelled value in a definition list. */
export function Fact({ label, value, mono = false }: { label: string; value: ReactNode; mono?: boolean }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className={mono ? "mono" : undefined}>{value}</dd>
    </div>
  );
}
