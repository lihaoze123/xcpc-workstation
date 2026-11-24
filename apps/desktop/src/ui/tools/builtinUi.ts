// Pre-bundled static references to the built-in official `ui.js` modules so they work
// in NSH release builds and headless tests where `new Function()` is disabled by NSH.
// Note: These are NEVER used to populate the Tools list (which is 100% dynamically driven
// by `plugin_list_tools()`), and local overrides / third-party plugins still load via
// `pluginUiSource()` + `mountPluginUi()`.

import type { JSX } from "nsh";
import type { PluginUiSdk } from "./pluginSdk";

// @ts-expect-error CommonJS `ui.js` module shipped in `crates/xcpc-plugin/plugins/graph-editor/ui.js`
import graphEditorUi from "../../../../../crates/xcpc-plugin/plugins/graph-editor/ui.js";
// @ts-expect-error CommonJS `ui.js` module shipped in `crates/xcpc-plugin/plugins/math-bitwise/ui.js`
import mathBitwiseUi from "../../../../../crates/xcpc-plugin/plugins/math-bitwise/ui.js";

type UiModule = { render?: (sdk: PluginUiSdk) => JSX.Element };

export const BUILTIN_UI_MODULES: Readonly<Record<string, UiModule>> = {
  "graph-editor": graphEditorUi as UiModule,
  "math-bitwise": mathBitwiseUi as UiModule,
};
