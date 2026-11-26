/**
 * Returns the injected `window.xcpc` capability bridge inside an XCPC Workstation WebView.
 * Throws if called outside an XCPC Workstation plugin WebView host.
 */
export function getXcpc() {
  if (typeof window !== "undefined" && window.xcpc) {
    return window.xcpc;
  }
  throw new Error(
    "@xcpc-workstation/plugin-sdk/ui: window.xcpc is not available (must run inside an XCPC Workstation plugin WebView)",
  );
}
