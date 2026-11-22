import { useCallback, useEffect, useState } from "react";

export type Appearance = "system" | "light" | "dark";

const STORAGE_KEY = "unifiedstream.appearance";
const DARK_QUERY = "(prefers-color-scheme: dark)";

function isAppearance(value: unknown): value is Appearance {
  return value === "system" || value === "light" || value === "dark";
}

// Storage can be missing, blocked, or throw (private contexts, cleared site data). A preference is
// a convenience, so every access degrades to the default instead of failing the interface.
function readStored(): Appearance {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    return isAppearance(value) ? value : "system";
  } catch {
    return "system";
  }
}

function writeStored(value: Appearance) {
  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
    /* keep the choice for this run only */
  }
}

function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

/**
 * The user's appearance choice and the theme it resolves to.
 *
 * The resolved value is written to `data-theme` on the root, which is what the stylesheet reads.
 * Following the system is live: the resolved theme changes when the operating system's does.
 */
export function useAppearance() {
  const [appearance, setAppearanceState] = useState<Appearance>(readStored);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const resolved: "light" | "dark" = appearance === "system" ? (systemDark ? "dark" : "light") : appearance;

  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
  }, [resolved]);

  const setAppearance = useCallback((next: Appearance) => {
    setAppearanceState(next);
    writeStored(next);
  }, []);

  return { appearance, resolved, setAppearance };
}
