// Multi-window lifecycle manager for the independent `XCPC 诊断` window (NSH 0.7).

import { currentWindow, isAvailable as desktopAvailable, Window, type WindowRef } from "nsh/desktop";
import { t } from "../i18n";
import { DiagnosticsWindowContent } from "./DiagnosticsWindow";
import {
  diagnosticsOpen,
  isO11yEnvEnabled,
  o11yLog,
  startLiveDiagnostics,
  stopLiveDiagnostics,
} from "./telemetry";

export const DIAGNOSTICS_WINDOW_ID = "diagnostics";

let diagWindowRef: WindowRef | null = null;

/** Opens (or focuses) the independent developer diagnostics window and activates Live Diagnostics. */
export function openDiagnosticsWindow(): void {
  if (desktopAvailable) {
    const existing = Window.get(DIAGNOSTICS_WINDOW_ID);
    if (existing) {
      existing.focus();
      if (!diagnosticsOpen()) startLiveDiagnostics();
      return;
    }
    startLiveDiagnostics();
    try {
      const ref = Window.open(DiagnosticsWindowContent, {
        id: DIAGNOSTICS_WINDOW_ID,
        title: t("o11y.windowTitle"),
        titleBar: { style: "hidden", height: 44 },
        defaultSize: { width: 1120, height: 740 },
        minSize: { width: 780, height: 520 },
        restoreBounds: true,
      });
      diagWindowRef = ref;
      ref.on("close", () => {
        if (diagWindowRef === ref) diagWindowRef = null;
        stopLiveDiagnostics();
      });
      o11yLog("info", "ui", "Diagnostics window opened");
    } catch (err) {
      stopLiveDiagnostics();
      o11yLog("error", "ui", "Failed to open diagnostics window", {
        error: err instanceof Error ? err.message : String(err),
      });
    }
    return;
  }

  // Headless / non-desktop fallback (tests): toggle live diagnostics state directly.
  startLiveDiagnostics();
}

/** Closes the diagnostics window if open and reverts cleanly to Always-On Ring mode. */
export function closeDiagnosticsWindow(): void {
  if (desktopAvailable) {
    const existing = Window.get(DIAGNOSTICS_WINDOW_ID) ?? diagWindowRef;
    diagWindowRef = null;
    if (existing) {
      try {
        existing.close();
      } catch {
        // Ignore if already closed.
      }
    }
  }
  stopLiveDiagnostics();
}

/** Toggles or focuses the diagnostics window. */
export function toggleDiagnosticsWindow(): void {
  if (diagnosticsOpen()) {
    const existing = desktopAvailable ? Window.get(DIAGNOSTICS_WINDOW_ID) : null;
    if (existing) {
      existing.focus();
      return;
    }
    closeDiagnosticsWindow();
  } else {
    openDiagnosticsWindow();
  }
}

/**
 * Binds the main window's close lifecycle so closing the main window also closes
 * the diagnostics window and lets the app exit cleanly per `QuitMode`.
 * Also auto-opens the diagnostics window when launched with `XCPC_O11Y=1`.
 */
export function bindDiagnosticsLifecycle(): void {
  if (desktopAvailable) {
    try {
      const mainWin = currentWindow();
      mainWin.on("close", () => {
        closeDiagnosticsWindow();
      });
    } catch {
      // Headless / test environment without desktop window host.
    }
  }

  if (isO11yEnvEnabled()) {
    setTimeout(() => {
      openDiagnosticsWindow();
    }, 50);
  }
}
