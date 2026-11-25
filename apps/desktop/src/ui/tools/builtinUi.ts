// Phase 4D: All official tool plugins now live in the external `xcpc-plugins` repository
// and load dynamically via `pluginUiSource()` + `mountPluginUi()`.
// Built-in plugins are restricted to `codeforces` and `atcoder` (headless OJ sources).

import type { JSX } from "nsh";
import type { PluginUiSdk } from "./pluginSdk";

type UiModule = { render?: (sdk: PluginUiSdk) => JSX.Element };

export const BUILTIN_UI_MODULES: Readonly<Record<string, UiModule>> = {};
