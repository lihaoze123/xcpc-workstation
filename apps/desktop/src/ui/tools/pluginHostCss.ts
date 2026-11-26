// Host-injected low-specificity base CSS for sandboxed WebView tool plugins.
// Keeps Windows WebView2 and Linux WebKitGTK visual craft aligned with the native shell:
// thin overlay-style scrollbars, native-feeling custom `<select>` chevrons, crisp font
// rendering, and non-wrapping buttons without overriding any plugin-specific rules.

const CHEVRON_LIGHT =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2371717a' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E\")";

const CHEVRON_DARK =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23a1a1aa' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E\")";

export const HOST_WEBVIEW_BASE_CSS = [
  `:root{color-scheme:light;}`,
  `:root[data-theme="dark"]{color-scheme:dark;}`,
  `:where(html,body){-webkit-font-smoothing:antialiased;text-rendering:optimizeLegibility;}`,
  `:where(button){font-family:inherit;white-space:nowrap;}`,
  `:where(input,textarea,select){font-family:inherit;}`,
  `:where(select){appearance:none;-webkit-appearance:none;padding-right:24px;background-image:${CHEVRON_LIGHT};background-repeat:no-repeat;background-position:right 7px center;background-size:12px 12px;cursor:pointer;}`,
  `:root[data-theme="dark"] :where(select){background-image:${CHEVRON_DARK};}`,
  `:where(*){scrollbar-width:thin;scrollbar-color:rgba(120,120,130,0.32) transparent;}`,
  `::-webkit-scrollbar{width:6px;height:6px;}`,
  `::-webkit-scrollbar-track{background:transparent;}`,
  `::-webkit-scrollbar-thumb{background:rgba(120,120,130,0.32);border-radius:9999px;}`,
  `::-webkit-scrollbar-thumb:hover{background:rgba(120,120,130,0.52);}`,
  `::-webkit-scrollbar-corner{background:transparent;}`,
].join("\n");
