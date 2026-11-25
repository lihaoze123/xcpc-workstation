//! Persistent QuickJS UI session engine for declarative plugin UIs (`ui.js`).
//!
//! Runs `ui.js` inside a sandboxed `rquickjs` runtime (64 MiB memory limit, 2s per-turn timeout)
//! with a transparent JSX / Signal / Canvas2D-recorder SDK, serializing the reactive component tree
//! into a lightweight JSON VNode tree for native NSH rendering without `eval` or `new Function()`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Exception, Function, Runtime};
use serde::{Deserialize, Serialize};

use crate::credentials::{CredentialStore, MemoryCredentialStore};
use crate::error::{PluginError, Result};
use crate::http::{HttpClient, UreqHttpClient};
use crate::kv::KvStore;
use crate::manager::LoadedPlugin;
use crate::render::svg_to_png_base64;
use crate::runtime::{HostContext, PluginRuntime, SandboxLimits};
use crate::workspace::WorkspaceReader;

pub const UI_SESSION_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub const UI_SESSION_TURN_TIMEOUT_MS: u64 = 2_000;

/// One host-side effect requested by the QuickJS UI session during mount or event handling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiHostEffect {
    pub id: u64,
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// One shortcut binding registered by the plugin via `sdk.onShortcut(...)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiShortcutSpec {
    pub id: String,
    pub keystroke: String,
    pub when: String,
}

/// Serialized output returned by every UI session turn (`mount`, `dispatch_event`, `resolve_effect`, `update_theme`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiRenderOutput {
    pub tree: serde_json::Value,
    #[serde(default)]
    pub effects: Vec<UiHostEffect>,
    #[serde(default)]
    pub shortcuts: Vec<UiShortcutSpec>,
}

const UI_SDK_BOOTSTRAP_JS: &str = r##"
(function() {
  const _kvGet = globalThis.__xcpc_ui_kv_get;
  const _kvSet = globalThis.__xcpc_ui_kv_set;
  const _kvDelete = globalThis.__xcpc_ui_kv_delete;
  const _runTool = globalThis.__xcpc_ui_run_tool;
  const _svgToPng = globalThis.__xcpc_ui_svg_to_png;
  const _wsHas = globalThis.__xcpc_ui_ws_has;
  const _wsList = globalThis.__xcpc_ui_ws_list;
  const _wsGet = globalThis.__xcpc_ui_ws_get;
  const _wsCurrent = globalThis.__xcpc_ui_ws_current;

  delete globalThis.__xcpc_ui_kv_get;
  delete globalThis.__xcpc_ui_kv_set;
  delete globalThis.__xcpc_ui_kv_delete;
  delete globalThis.__xcpc_ui_run_tool;
  delete globalThis.__xcpc_ui_svg_to_png;
  delete globalThis.__xcpc_ui_ws_has;
  delete globalThis.__xcpc_ui_ws_list;
  delete globalThis.__xcpc_ui_ws_get;
  delete globalThis.__xcpc_ui_ws_current;

  // --- Lightweight reactive primitives (signal / computed / effect / untracked) ---
  let activeObserver = null;
  let allEffects = [];

  function signal(initial) {
    let value = initial;
    const subs = new Set();
    const read = function() {
      if (activeObserver) subs.add(activeObserver);
      return value;
    };
    read.set = function(next) {
      const resolved = typeof next === "function" ? next(value) : next;
      if (Object.is(resolved, value)) return;
      value = resolved;
      const toRun = Array.from(subs);
      for (const fn of toRun) {
        try { fn(); } catch (_) {}
      }
    };
    read.update = function(updater) {
      read.set(updater(value));
    };
    return read;
  }

  function computed(fn) {
    return function() {
      return fn();
    };
  }

  function effect(fn) {
    let disposed = false;
    const runner = function() {
      if (disposed) return;
      const prev = activeObserver;
      activeObserver = runner;
      try { fn(); } finally { activeObserver = prev; }
    };
    allEffects.push(runner);
    runner();
    return function() {
      disposed = true;
    };
  }

  function untracked(fn) {
    const prev = activeObserver;
    activeObserver = null;
    try { return fn(); } finally { activeObserver = prev; }
  }

  // --- Theme & host state ---
  let themeTokens = {};
  let currentColorScheme = "light";
  let initialPayloadValue = undefined;
  let cleanups = [];
  let pendingEffects = [];
  let nextEffectId = 1;
  let effectResolvers = new Map();
  let registeredShortcuts = [];
  let shortcutCallbacks = new Map();
  let nextShortcutId = 1;

  function queueEffect(kind, payload, expectReply) {
    const id = nextEffectId++;
    pendingEffects.push({ id, kind, payload: payload ?? {} });
    if (!expectReply) return undefined;
    return new Promise((resolve, reject) => {
      effectResolvers.set(id, { resolve, reject });
    });
  }

  // --- Canvas2D command recorder ---
  function createCanvasRecorder() {
    const ops = [];
    const ctx = {
      _fillStyle: "#000000",
      _strokeStyle: "#000000",
      _lineWidth: 1,
      _font: "12px sans-serif",
      _textAlign: "left",
      _textBaseline: "alphabetic",
      _globalAlpha: 1,
      get fillStyle() { return this._fillStyle; },
      set fillStyle(v) { this._fillStyle = String(v); ops.push(["fillStyle", this._fillStyle]); },
      get strokeStyle() { return this._strokeStyle; },
      set strokeStyle(v) { this._strokeStyle = String(v); ops.push(["strokeStyle", this._strokeStyle]); },
      get lineWidth() { return this._lineWidth; },
      set lineWidth(v) { this._lineWidth = Number(v); ops.push(["lineWidth", this._lineWidth]); },
      get font() { return this._font; },
      set font(v) { this._font = String(v); ops.push(["font", this._font]); },
      get textAlign() { return this._textAlign; },
      set textAlign(v) { this._textAlign = String(v); ops.push(["textAlign", this._textAlign]); },
      get textBaseline() { return this._textBaseline; },
      set textBaseline(v) { this._textBaseline = String(v); ops.push(["textBaseline", this._textBaseline]); },
      get globalAlpha() { return this._globalAlpha; },
      set globalAlpha(v) { this._globalAlpha = Number(v); ops.push(["globalAlpha", this._globalAlpha]); },
      clearRect(x, y, w, h) { ops.push(["clearRect", +x, +y, +w, +h]); },
      fillRect(x, y, w, h) { ops.push(["fillRect", +x, +y, +w, +h]); },
      strokeRect(x, y, w, h) { ops.push(["strokeRect", +x, +y, +w, +h]); },
      beginPath() { ops.push(["beginPath"]); },
      closePath() { ops.push(["closePath"]); },
      moveTo(x, y) { ops.push(["moveTo", +x, +y]); },
      lineTo(x, y) { ops.push(["lineTo", +x, +y]); },
      arc(x, y, r, s, e, ccw) { ops.push(["arc", +x, +y, +r, +s, +e, Boolean(ccw)]); },
      stroke() { ops.push(["stroke"]); },
      fill() { ops.push(["fill"]); },
      fillText(text, x, y) { ops.push(["fillText", String(text), +x, +y]); },
      strokeText(text, x, y) { ops.push(["strokeText", String(text), +x, +y]); },
      save() { ops.push(["save"]); },
      restore() { ops.push(["restore"]); },
      translate(x, y) { ops.push(["translate", +x, +y]); },
      scale(x, y) { ops.push(["scale", +x, +y]); },
      rotate(a) { ops.push(["rotate", +a]); },
      setLineDash(seg) { ops.push(["setLineDash", Array.isArray(seg) ? seg.map(Number) : []]); },
    };
    return { ctx, ops };
  }

  function fitBounds(points, viewport, padding) {
    const pad = padding == null ? 32 : Number(padding);
    const w = Math.max(1, Number(viewport && viewport.width) || 1);
    const h = Math.max(1, Number(viewport && viewport.height) || 1);
    if (!Array.isArray(points) || points.length === 0) {
      return { scale: 1, offsetX: w / 2, offsetY: h / 2, centerX: 0, centerY: 0 };
    }
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const pt of points) {
      if (pt.x < minX) minX = pt.x;
      if (pt.x > maxX) maxX = pt.x;
      if (pt.y < minY) minY = pt.y;
      if (pt.y > maxY) maxY = pt.y;
    }
    const centerX = (minX + maxX) / 2;
    const centerY = (minY + maxY) / 2;
    const spanX = Math.max(1e-6, maxX - minX);
    const spanY = Math.max(1e-6, maxY - minY);
    const availW = Math.max(1, w - pad * 2);
    const availH = Math.max(1, h - pad * 2);
    const scale = points.length === 1
      ? 1
      : Math.max(0.05, Math.min(4, Math.min(availW / spanX, availH / spanY)));
    return {
      scale,
      offsetX: w / 2 - centerX * scale,
      offsetY: h / 2 - centerY * scale,
      centerX,
      centerY,
    };
  }

  // --- Component tags & VNode factory ---
  const UI_TYPES = [
    "View", "Text", "Scroll", "Canvas", "Button", "Icon",
    "Segmented", "Chip", "Input", "If", "Each", "Row", "Col",
    "Card", "Switch", "Select", "Badge", "Menu", "Modal",
    "Image", "CodeEditor", "Output",
  ];
  const ui = {};
  for (const name of UI_TYPES) {
    ui[name] = name;
  }

  function Fragment(props) {
    return props ? props.children : null;
  }

  function h(type, props, ...rawChildren) {
    const merged = props ? Object.assign({}, props) : {};
    if (rawChildren.length === 1) {
      merged.children = rawChildren[0];
    } else if (rawChildren.length > 1) {
      merged.children = rawChildren;
    }
    return { __vnode: true, type, props: merged };
  }

  // --- Callback registry & VNode serializer ---
  let callbackTable = new Map();
  let canvasSizes = new Map();
  let nextCbId = 1;

  function registerCb(fn) {
    const id = "cb_" + (nextCbId++);
    callbackTable.set(id, fn);
    return id;
  }

  function resolveMaybe(v) {
    return typeof v === "function" ? v() : v;
  }

  function resolveText(ch) {
    if (typeof ch === "function") return resolveText(ch());
    if (Array.isArray(ch)) return ch.map(resolveText).join("");
    if (ch === null || ch === undefined || typeof ch === "boolean") return "";
    if (typeof ch === "object" && ch.__vnode) return "";
    return String(ch);
  }

  const EVENT_PROPS = new Set([
    "onPress", "onChange", "onChangeText", "onSelect", "onClose",
    "onLayout", "onPressDown", "onMouseMove", "onMouseUp", "onMouseLeave",
  ]);

  function serializeChildren(raw, pathPrefix) {
    const out = [];
    const pushItem = (item, idx) => {
      if (item === null || item === undefined || typeof item === "boolean") return;
      if (typeof item === "function") {
        pushItem(item(), idx);
        return;
      }
      if (Array.isArray(item)) {
        for (let i = 0; i < item.length; i++) {
          pushItem(item[i], idx + "_" + i);
        }
        return;
      }
      if (typeof item === "object" && item.__vnode) {
        const ser = serializeNode(item, pathPrefix + "." + idx);
        if (ser !== null) {
          if (Array.isArray(ser)) {
            for (const s of ser) if (s !== null) out.push(s);
          } else {
            out.push(ser);
          }
        }
        return;
      }
      out.push({ type: "Text", props: { text: String(item) }, children: [] });
    };
    pushItem(raw, "0");
    return out;
  }

  function serializeNode(node, path) {
    if (!node || typeof node !== "object") return null;
    if (typeof node === "function") return serializeNode(node(), path);
    if (!node.__vnode) return null;

    const { type, props } = node;
    if (typeof type === "function") {
      if (type === Fragment) {
        return serializeChildren(props.children, path);
      }
      const rendered = type(props || {});
      return serializeNode(rendered, path + ":fn");
    }

    // Special control-flow components evaluated right inside QuickJS!
    if (type === "If") {
      const cond = resolveMaybe(props.when);
      if (!cond) {
        return props.else ? serializeChildren(props.else, path + ":else") : null;
      }
      const ch = props.children;
      if (typeof ch === "function") {
        return serializeChildren(ch(cond), path + ":then");
      }
      return serializeChildren(ch, path + ":then");
    }

    if (type === "Each") {
      const list = resolveMaybe(props.of) || [];
      const out = [];
      for (let i = 0; i < list.length; i++) {
        const item = list[i];
        const idx = i;
        if (typeof props.children === "function") {
          const el = props.children(() => item, () => idx);
          const ser = serializeNode(el, path + ":each:" + i);
          if (ser !== null) {
            if (Array.isArray(ser)) out.push(...ser);
            else out.push(ser);
          }
        }
      }
      return out;
    }

    const outProps = {};
    for (const k of Object.keys(props || {})) {
      if (k === "children" || k === "draw" || k === "trigger") continue;
      const rawVal = props[k];
      if (rawVal === undefined) continue;
      if (EVENT_PROPS.has(k) && typeof rawVal === "function") {
        outProps[k] = registerCb(rawVal);
      } else if (typeof rawVal === "function") {
        outProps[k] = rawVal();
      } else {
        outProps[k] = rawVal;
      }
    }

    if (type === "Menu" && props.trigger) {
      outProps.triggerNode = serializeNode(props.trigger, path + ":trigger");
    }

    if (type === "Canvas") {
      const canvasKey = props.testID || path;
      outProps.__canvasKey = canvasKey;
      const sz = canvasSizes.get(canvasKey) || { width: 420, height: 420 };
      if (typeof props.draw === "function") {
        const rec = createCanvasRecorder();
        try {
          props.draw(rec.ctx, sz);
        } catch (err) {
          // Record draw error silently so host canvas stays clean.
        }
        outProps.drawOps = rec.ops;
      }
      // Wrap onLayout to remember canvas dimensions inside QuickJS as well.
      const userOnLayout = typeof props.onLayout === "function" ? props.onLayout : null;
      outProps.onLayout = registerCb((ev) => {
        const w = ev && ev.bounds && ev.bounds.size ? ev.bounds.size.width : 0;
        const hVal = ev && ev.bounds && ev.bounds.size ? ev.bounds.size.height : 0;
        if (w > 10 && hVal > 10) {
          canvasSizes.set(canvasKey, { width: w, height: hVal });
        }
        if (userOnLayout) userOnLayout(ev);
      });
      return { type, props: outProps, children: [] };
    }

    if (type === "Text") {
      outProps.text = resolveText(props.children);
      return { type, props: outProps, children: [] };
    }

    if (type === "Button" || type === "Badge") {
      const ch = resolveMaybe(props.children);
      if (typeof ch === "string" || typeof ch === "number") {
        outProps.label = String(ch);
        return { type, props: outProps, children: [] };
      }
      return { type, props: outProps, children: serializeChildren(ch, path) };
    }

    return {
      type,
      props: outProps,
      children: serializeChildren(props.children, path),
    };
  }

  // --- SDK object exposed to `ui.js` ---
  const storage = Object.freeze({
    get(key) {
      const raw = _kvGet(String(key));
      return raw == null ? undefined : JSON.parse(raw);
    },
    set(key, val) {
      _kvSet(String(key), JSON.stringify(val === undefined ? null : val));
    },
    delete(key) {
      return Boolean(_kvDelete(String(key)));
    },
  });

  const workspace = Object.freeze({
    hasWorkspace() { return Boolean(_wsHas()); },
    currentProblemId() {
      const cur = _wsCurrent();
      return cur == null ? undefined : String(cur);
    },
    listProblems() { return JSON.parse(_wsList()); },
    getProblem(id) { return JSON.parse(_wsGet(String(id))); },
  });

  function runTool(toolId, inputs) {
    try {
      const raw = _runTool(String(toolId), JSON.stringify(inputs || {}));
      return Promise.resolve(JSON.parse(raw));
    } catch (err) {
      return Promise.reject(err);
    }
  }

  function useTool(toolId, inputsAccessor, options) {
    const data = signal(undefined);
    const loading = signal(false);
    const error = signal(undefined);
    const refreshTick = signal(0);
    let lastKey = null;

    const runNow = () => {
      const enabled = options && options.enabled !== undefined
        ? Boolean(resolveMaybe(options.enabled))
        : true;
      if (!enabled) {
        loading.set(false);
        return;
      }
      const payload = resolveMaybe(inputsAccessor) || {};
      const key = JSON.stringify(payload) + "#" + refreshTick();
      if (key === lastKey && data() !== undefined) return;
      lastKey = key;
      try {
        const raw = _runTool(String(toolId), JSON.stringify(payload));
        data.set(JSON.parse(raw));
        error.set(undefined);
        loading.set(false);
      } catch (err) {
        error.set(err && err.message ? err.message : String(err));
        loading.set(false);
      }
    };

    effect(() => {
      void refreshTick();
      runNow();
    });

    return {
      data,
      loading,
      error,
      refresh() {
        lastKey = null;
        refreshTick.set(refreshTick() + 1);
      },
    };
  }

  const sdk = {
    h,
    Fragment,
    signal,
    computed,
    effect,
    untracked,
    ui,
    canvas: { fitBounds },
    token(name) {
      return themeTokens[name] || "#888888";
    },
    colorScheme() {
      return currentColorScheme;
    },
    runTool,
    useTool,
    onCleanup(fn) {
      if (typeof fn === "function") cleanups.push(fn);
    },
    onShortcut(keystroke, handler, options) {
      const id = "sc_" + (nextShortcutId++);
      const when = (options && options.when) || "notEditing";
      registeredShortcuts.push({ id, keystroke: String(keystroke), when });
      shortcutCallbacks.set(id, handler);
      return function unbind() {
        shortcutCallbacks.delete(id);
        registeredShortcuts = registeredShortcuts.filter((s) => s.id !== id);
      };
    },
    storage,
    workspace,
    openTool(pluginId, payload) {
      queueEffect("openTool", { pluginId: String(pluginId), payload }, false);
    },
    pickFile(options) {
      return queueEffect("pickFile", options || {}, true);
    },
    readClipboard() {
      return queueEffect("readClipboard", {}, true);
    },
    openExternal(url) {
      const trimmed = String(url || "").trim();
      if (!/^https?:\/\//i.test(trimmed)) {
        throw new Error("openExternal only permits http:// and https:// URLs");
      }
      queueEffect("openExternal", { url: trimmed }, false);
      return true;
    },
    renderSvgToPng(svg, scale) {
      try {
        const b64 = _svgToPng(String(svg || ""), scale == null ? 2.0 : Number(scale));
        return Promise.resolve(b64);
      } catch (err) {
        return Promise.reject(err);
      }
    },
    savePng(pngBase64, defaultName) {
      return queueEffect("savePng", { pngBase64: String(pngBase64 || ""), defaultName: defaultName || "output.png" }, true);
    },
    saveText(text, defaultName) {
      return queueEffect("saveText", { text: String(text || ""), defaultName: defaultName || "output.txt" }, true);
    },
    copyText(text) {
      queueEffect("copyText", { text: String(text || "") }, false);
      return Promise.resolve();
    },
    toast(text, tone) {
      queueEffect("toast", { text: String(text || ""), tone: tone || "success" }, false);
    },
    initialPayload() {
      return initialPayloadValue;
    },
  };

  let rootVNode = null;

  function flushSnapshot() {
    callbackTable = new Map();
    nextCbId = 1;
    const tree = untracked(() => serializeNode(rootVNode, "root"));
    const effects = pendingEffects;
    pendingEffects = [];
    return JSON.stringify({
      tree: tree || { type: "View", props: {}, children: [] },
      effects,
      shortcuts: registeredShortcuts,
    });
  }

  globalThis.__xcpc_ui_sdk = sdk;
  globalThis.__xcpc_ui_flush = flushSnapshot;
  globalThis.__xcpc_ui_mount_only = function(payloadJson, themeJson, scheme) {
    themeTokens = themeJson ? JSON.parse(themeJson) : {};
    currentColorScheme = scheme || "light";
    initialPayloadValue = payloadJson ? JSON.parse(payloadJson) : undefined;
    const mod = globalThis.__xcpc_ui_exports || {};
    const renderFn = typeof mod.render === "function"
      ? mod.render
      : (typeof mod.default === "function" ? mod.default : null);
    if (!renderFn) {
      throw new Error("ui.js must export a render(sdk) function");
    }
    rootVNode = untracked(() => renderFn(sdk));
  };

  globalThis.__xcpc_ui_invoke = function(cbId, payloadJson) {
    const payload = payloadJson ? JSON.parse(payloadJson) : undefined;
    const fn = callbackTable.get(cbId) || shortcutCallbacks.get(cbId);
    if (typeof fn === "function") {
      fn(payload);
    }
  };

  globalThis.__xcpc_ui_resolve_only = function(effectId, resultJson) {
    const entry = effectResolvers.get(effectId);
    if (entry) {
      effectResolvers.delete(effectId);
      const parsed = resultJson ? JSON.parse(resultJson) : {};
      if (parsed && parsed.error) {
        entry.reject(new Error(String(parsed.error)));
      } else {
        entry.resolve(parsed ? parsed.value : undefined);
      }
    }
  };

  globalThis.__xcpc_ui_update_theme = function(themeJson, scheme) {
    themeTokens = themeJson ? JSON.parse(themeJson) : {};
    currentColorScheme = scheme || "light";
  };

  globalThis.__xcpc_ui_dispose = function() {
    for (let i = cleanups.length - 1; i >= 0; i--) {
      try { cleanups[i](); } catch (_) {}
    }
    cleanups = [];
    callbackTable.clear();
    shortcutCallbacks.clear();
    effectResolvers.clear();
  };
})();
"##;

/// A persistent, sandboxed QuickJS session hosting one desktop plugin's `ui.js`.
pub struct PluginUiSession {
    plugin_id: String,
    rt: Runtime,
    ctx: Context,
    deadline: Arc<Mutex<Instant>>,
    timed_out: Arc<AtomicBool>,
}

/// Optional workspace context hooks supplied by the desktop host to a UI session.
pub trait UiWorkspaceBridge: Send + Sync {
    fn has_workspace(&self) -> bool;
    fn current_problem_id(&self) -> Option<String>;
    fn list_problems_json(&self) -> String;
    fn get_problem_json(&self, id: &str) -> String;
}

impl PluginUiSession {
    /// Creates and mounts a new sandboxed QuickJS UI session for `loaded`.
    #[allow(clippy::too_many_arguments)]
    pub fn mount(
        loaded: LoadedPlugin,
        kv: Arc<KvStore>,
        http_client: Option<Arc<dyn HttpClient>>,
        credentials: Option<Arc<dyn CredentialStore>>,
        workspace_reader: Option<Arc<dyn WorkspaceReader>>,
        ui_workspace: Option<Arc<dyn UiWorkspaceBridge>>,
        initial_payload_json: Option<&str>,
        theme_tokens_json: &str,
        color_scheme: &str,
    ) -> Result<(Self, UiRenderOutput)> {
        let ui_source = loaded.ui_source.clone().ok_or_else(|| {
            PluginError::InvalidPackage(format!(
                "plugin '{}' has no ui_entry configured",
                loaded.info.manifest.id
            ))
        })?;
        let plugin_id = loaded.info.manifest.id.clone();

        let rt = Runtime::new().map_err(|e| PluginError::JsError(e.to_string()))?;
        rt.set_memory_limit(UI_SESSION_MEMORY_LIMIT_BYTES);
        rt.set_max_stack_size(1024 * 1024);

        let deadline = Arc::new(Mutex::new(
            Instant::now() + Duration::from_millis(UI_SESSION_TURN_TIMEOUT_MS),
        ));
        let timed_out = Arc::new(AtomicBool::new(false));
        let deadline_check = Arc::clone(&deadline);
        let timed_out_flag = Arc::clone(&timed_out);
        rt.set_interrupt_handler(Some(Box::new(move || {
            let expired = deadline_check
                .lock()
                .map(|d| Instant::now() >= *d)
                .unwrap_or(false);
            if expired {
                timed_out_flag.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        let ctx = Context::full(&rt).map_err(|e| PluginError::JsError(e.to_string()))?;

        let entry_source = Arc::new(loaded.entry_source.clone());
        let manifest = loaded.info.manifest.clone();
        let http_arc: Arc<dyn HttpClient> =
            http_client.unwrap_or_else(|| Arc::new(UreqHttpClient::new()));
        let cred_arc: Arc<dyn CredentialStore> =
            credentials.unwrap_or_else(|| Arc::new(MemoryCredentialStore::default()));

        let ui_filename = format!(
            "xcpc-plugin://{}/{}",
            plugin_id,
            manifest.ui_entry.as_deref().unwrap_or("ui.js")
        );

        ctx.with(|ctx| {
            register_ui_bridges(
                &ctx,
                &manifest,
                &entry_source,
                &kv,
                &http_arc,
                &cred_arc,
                &workspace_reader,
                &ui_workspace,
            )?;

            // Bootstrap SDK runtime.
            eval_void(&ctx, UI_SDK_BOOTSTRAP_JS, "xcpc-ui-bootstrap.js")?;

            // Load `ui.js` passing `sdk` directly into module closure (matches Vite virtual module contract).
            let wrapped = format!(
                "globalThis.__xcpc_ui_exports = (function(sdk) {{ const exports = {{}}; const module = {{ exports }}; (function(sdk, module, exports) {{{ui_source}\n}})(sdk, module, exports); return module.exports; }})(globalThis.__xcpc_ui_sdk);"
            );
            eval_void(&ctx, &wrapped, &ui_filename)?;

            let payload_arg = serde_json::to_string(&initial_payload_json.unwrap_or(""))?;
            let theme_arg = serde_json::to_string(theme_tokens_json)?;
            let scheme_arg = serde_json::to_string(color_scheme)?;
            let call = format!(
                "globalThis.__xcpc_ui_mount_only({payload_arg}, {theme_arg}, {scheme_arg});"
            );
            eval_void(&ctx, &call, "mount.js")
        })?;

        let session = Self {
            plugin_id,
            rt,
            ctx,
            deadline,
            timed_out,
        };
        session.drain_jobs()?;
        let output = session.flush()?;
        Ok((session, output))
    }

    fn flush(&self) -> Result<UiRenderOutput> {
        let raw = self
            .ctx
            .with(|ctx| eval_string(&ctx, "globalThis.__xcpc_ui_flush()"))?;
        let output: UiRenderOutput = serde_json::from_str(&raw)?;
        Ok(output)
    }

    fn reset_deadline(&self) {
        self.timed_out.store(false, Ordering::SeqCst);
        if let Ok(mut d) = self.deadline.lock() {
            *d = Instant::now() + Duration::from_millis(UI_SESSION_TURN_TIMEOUT_MS);
        }
    }

    fn drain_jobs(&self) -> Result<()> {
        while self.rt.is_job_pending() {
            if self.timed_out.load(Ordering::SeqCst) {
                return Err(PluginError::Timeout {
                    plugin_id: self.plugin_id.clone(),
                    timeout_ms: UI_SESSION_TURN_TIMEOUT_MS,
                });
            }
            match self.rt.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => return Err(PluginError::JsError(e.to_string())),
            }
        }
        Ok(())
    }

    /// Dispatches a UI callback or registered shortcut event and returns the refreshed VNode tree + effects.
    pub fn dispatch_event(
        &self,
        callback_id: &str,
        payload_json: Option<&str>,
    ) -> Result<UiRenderOutput> {
        self.reset_deadline();
        let cb_arg = serde_json::to_string(callback_id)?;
        let payload_arg = serde_json::to_string(&payload_json.unwrap_or(""))?;
        let call = format!("globalThis.__xcpc_ui_invoke({cb_arg}, {payload_arg});");
        self.ctx.with(|ctx| eval_void(&ctx, &call, "dispatch.js"))?;
        self.drain_jobs()?;
        self.flush()
    }

    /// Resolves an async host effect (e.g. `pickFile`, `readClipboard`, `savePng`) back into the QuickJS session.
    pub fn resolve_effect(&self, effect_id: u64, result_json: &str) -> Result<UiRenderOutput> {
        self.reset_deadline();
        let res_arg = serde_json::to_string(result_json)?;
        let call = format!("globalThis.__xcpc_ui_resolve_only({effect_id}, {res_arg});");
        self.ctx.with(|ctx| eval_void(&ctx, &call, "resolve.js"))?;
        self.drain_jobs()?;
        self.flush()
    }

    /// Updates theme tokens and color scheme (e.g. light <-> dark toggle) and returns the re-rendered tree.
    pub fn update_theme(
        &self,
        theme_tokens_json: &str,
        color_scheme: &str,
    ) -> Result<UiRenderOutput> {
        self.reset_deadline();
        let theme_arg = serde_json::to_string(theme_tokens_json)?;
        let scheme_arg = serde_json::to_string(color_scheme)?;
        let call = format!("globalThis.__xcpc_ui_update_theme({theme_arg}, {scheme_arg});");
        self.ctx.with(|ctx| eval_void(&ctx, &call, "theme.js"))?;
        self.drain_jobs()?;
        self.flush()
    }

    /// Invokes all registered `sdk.onCleanup` callbacks in reverse order.
    pub fn dispose(&self) {
        self.reset_deadline();
        let _ = self.ctx.with(|ctx| {
            eval_void(
                &ctx,
                "if (globalThis.__xcpc_ui_dispose) globalThis.__xcpc_ui_dispose();",
                "dispose.js",
            )
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn register_ui_bridges<'js>(
    ctx: &Ctx<'js>,
    manifest: &crate::manifest::PluginManifest,
    entry_source: &Arc<String>,
    kv: &Arc<KvStore>,
    http_client: &Arc<dyn HttpClient>,
    credentials: &Arc<dyn CredentialStore>,
    workspace_reader: &Option<Arc<dyn WorkspaceReader>>,
    ui_workspace: &Option<Arc<dyn UiWorkspaceBridge>>,
) -> Result<()> {
    let globals = ctx.globals();

    // KV bridges
    {
        let kv_get = Arc::clone(kv);
        let f = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, key: String| match kv_get.get(&key) {
                Ok(Some(v)) => serde_json::to_string(&v)
                    .map(Some)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                Ok(None) => Ok(None),
                Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_kv_get", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }
    {
        let kv_set = Arc::clone(kv);
        let f = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, key: String, val_json: String| {
                let val: serde_json::Value = serde_json::from_str(&val_json)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
                kv_set
                    .set(&key, val)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_kv_set", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }
    {
        let kv_del = Arc::clone(kv);
        let f = Function::new(ctx.clone(), move |ctx: Ctx<'js>, key: String| {
            kv_del
                .delete(&key)
                .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_kv_delete", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // Tool execution bridge (invokes same plugin's `index.js` `runTool` inside its sandbox)
    {
        let m_clone = manifest.clone();
        let src_clone = Arc::clone(entry_source);
        let http_clone = Arc::clone(http_client);
        let cred_clone = Arc::clone(credentials);
        let kv_clone = Arc::clone(kv);
        let ws_clone = workspace_reader.clone();
        let f = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, tool_id: String, inputs_json: String| {
                let inputs: serde_json::Value = serde_json::from_str(&inputs_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                let tool_ctx = serde_json::json!({
                    "plugin_id": m_clone.id,
                    "tool_id": tool_id,
                });
                let host_ctx = HostContext {
                    manifest: m_clone.clone(),
                    http_client: Arc::clone(&http_clone),
                    credentials: Arc::clone(&cred_clone),
                    kv: Arc::clone(&kv_clone),
                    workspace: ws_clone.clone(),
                    limits: SandboxLimits::default(),
                };
                match PluginRuntime::call_function(
                    &src_clone,
                    "runTool",
                    &[serde_json::Value::String(tool_id), inputs, tool_ctx],
                    &host_ctx,
                ) {
                    Ok(call) => serde_json::to_string(&call.value)
                        .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                    Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_run_tool", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // SVG -> PNG bridge
    {
        let f = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, svg: String, scale: f64| {
                svg_to_png_base64(&svg, Some(scale as f32))
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_svg_to_png", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // Read-only workspace bridge (gated by `manifest.permissions.workspace_read`)
    let allow_ws = manifest.permissions.workspace_read;
    {
        let ws = ui_workspace.clone();
        let f = Function::new(ctx.clone(), move || {
            allow_ws && ws.as_ref().is_some_and(|w| w.has_workspace())
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_ws_has", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }
    {
        let ws = ui_workspace.clone();
        let f = Function::new(ctx.clone(), move || -> Option<String> {
            if !allow_ws {
                return None;
            }
            ws.as_ref().and_then(|w| w.current_problem_id())
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_ws_current", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }
    {
        let ws = ui_workspace.clone();
        let f = Function::new(ctx.clone(), move || -> String {
            if !allow_ws {
                return "[]".to_string();
            }
            ws.as_ref()
                .map(|w| w.list_problems_json())
                .unwrap_or_else(|| "[]".to_string())
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_ws_list", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }
    {
        let ws = ui_workspace.clone();
        let f = Function::new(ctx.clone(), move |id: String| -> String {
            if !allow_ws {
                return "null".to_string();
            }
            ws.as_ref()
                .map(|w| w.get_problem_json(&id))
                .unwrap_or_else(|| "null".to_string())
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ui_ws_get", f)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    Ok(())
}

fn format_caught<'js>(err: CaughtError<'js>) -> String {
    match err {
        CaughtError::Exception(exc) => {
            let msg = exc
                .message()
                .unwrap_or_else(|| "JavaScript exception".to_string());
            if let Some(stack) = exc.stack() {
                let st = stack.trim();
                if !st.is_empty() && !msg.contains(st) {
                    return format!("{msg}\n{st}");
                }
            }
            msg
        }
        other => format!("{other}"),
    }
}

fn eval_void<'js>(ctx: &Ctx<'js>, code: &str, filename: &str) -> Result<()> {
    let mut opts = EvalOptions::default();
    opts.global = true;
    opts.filename = Some(filename.to_string());
    ctx.eval_with_options::<(), _>(code, opts)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught(e)))
}

fn eval_string<'js>(ctx: &Ctx<'js>, code: &str) -> Result<String> {
    ctx.eval::<String, _>(code)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught(e)))
}
