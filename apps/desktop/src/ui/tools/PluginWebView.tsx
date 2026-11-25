// Pure-Web `<PluginWebView>` host for custom UI plugins (`ui_entry = "ui.html"`).
// Loads raw HTML via `preview.loadPluginUiHtml(pluginId)`, injects a strict CSP,
// optional host theme CSS variables, and the `window.xcpc` capability bridge over
// `window.nsh.postMessage`.

import {
  cleanup,
  Clipboard,
  colorScheme,
  effect,
  signal,
  Text,
  untracked,
  View,
  WebView,
  type WebViewRef,
} from "nsh";
import { Preview } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { Btn } from "../kit";
import { goSettings, openToolPlugin, pluginRevisionFor, preview } from "../store";
import { MONO, token, UI_SANS } from "../theme";
import { showToast, type ToastTone } from "../Toast";
import { zhEn } from "../training/locale";
import { classifyPluginUiError, formatPluginError, type PluginUiErrorInfo } from "./pluginErrors";

export const PLUGIN_WEBVIEW_CSP =
  "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';";

const DATA_IMAGE_BASE64_RE = /^data:image\/[a-zA-Z0-9.+-]+;base64,([A-Za-z0-9+/=\s]+)$/;
const RAW_BASE64_RE = /^[A-Za-z0-9+/=\s]+$/;

/**
 * Normalizes a plugin `clipboard.writeImage` argument into a safe `data:image/...;base64,...` URL.
 * Accepts either raw PNG base64 (matching `files.savePng`) or a base64 `data:image/...` URL
 * (such as `canvas.toDataURL("image/png")`), and rejects arbitrary URL schemes.
 */
export function normalizeClipboardImageSrc(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new Error("Empty image data");
  }
  const dataMatch = DATA_IMAGE_BASE64_RE.exec(trimmed);
  if (dataMatch) {
    const payload = dataMatch[1].replace(/\s+/g, "");
    if (!payload) throw new Error("Empty image data");
    const prefix = trimmed.slice(0, trimmed.indexOf(",") + 1);
    return `${prefix}${payload}`;
  }
  if (trimmed.includes(":") || !RAW_BASE64_RE.test(trimmed)) {
    throw new Error("Invalid image data: expected PNG base64 or data:image/...;base64,... URL");
  }
  return `data:image/png;base64,${trimmed.replace(/\s+/g, "")}`;
}

const FALLBACK_LIGHT_VARS: Record<string, string> = {
  "--xcpc-bg": "#FAFAFA",
  "--xcpc-fg": "#09090B",
  "--xcpc-card": "#FFFFFF",
  "--xcpc-card-fg": "#09090B",
  "--xcpc-muted": "#F4F4F5",
  "--xcpc-muted-fg": "#A1A1AA",
  "--xcpc-secondary-fg": "#52525B",
  "--xcpc-border": "#E4E4E7",
  "--xcpc-input": "#D4D4D8",
  "--xcpc-primary": "#0B6989",
  "--xcpc-primary-fg": "#FFFFFF",
  "--xcpc-accent": "#EBEBED",
  "--xcpc-destructive": "#B91C1C",
  "--xcpc-success": "#15803D",
  "--xcpc-warning": "#B45309",
  "--xcpc-font-sans": UI_SANS,
  "--xcpc-font-mono": MONO,
};

const FALLBACK_DARK_VARS: Record<string, string> = {
  "--xcpc-bg": "#09090B",
  "--xcpc-fg": "#FAFAFA",
  "--xcpc-card": "#111113",
  "--xcpc-card-fg": "#FAFAFA",
  "--xcpc-muted": "#18181B",
  "--xcpc-muted-fg": "#71717A",
  "--xcpc-secondary-fg": "#A1A1AA",
  "--xcpc-border": "#27272A",
  "--xcpc-input": "#3F3F46",
  "--xcpc-primary": "#0E7FA6",
  "--xcpc-primary-fg": "#FFFFFF",
  "--xcpc-accent": "#232327",
  "--xcpc-destructive": "#F87171",
  "--xcpc-success": "#4ADE80",
  "--xcpc-warning": "#FBBF24",
  "--xcpc-font-sans": UI_SANS,
  "--xcpc-font-mono": MONO,
};

type ThemeTokenKey = Parameters<typeof token>[0];

function safeToken(name: ThemeTokenKey, fallback: string): string {
  try {
    const val = token(name);
    return val && typeof val === "string" ? val : fallback;
  } catch {
    return fallback;
  }
}

/** Builds optional `--xcpc-*` CSS custom properties matching the host's active palette. */
export function buildThemeCssVars(scheme: "light" | "dark"): Record<string, string> {
  const fb = scheme === "dark" ? FALLBACK_DARK_VARS : FALLBACK_LIGHT_VARS;
  const activeScheme: "light" | "dark" = colorScheme() === "dark" ? "dark" : "light";
  if (scheme !== activeScheme) {
    return { ...fb };
  }
  return {
    "--xcpc-bg": safeToken("background", fb["--xcpc-bg"]),
    "--xcpc-fg": safeToken("foreground", fb["--xcpc-fg"]),
    "--xcpc-card": safeToken("card", fb["--xcpc-card"]),
    "--xcpc-card-fg": safeToken("card-foreground", fb["--xcpc-card-fg"]),
    "--xcpc-muted": safeToken("muted", fb["--xcpc-muted"]),
    "--xcpc-muted-fg": safeToken("muted-foreground", fb["--xcpc-muted-fg"]),
    "--xcpc-secondary-fg": safeToken("secondary-foreground", fb["--xcpc-secondary-fg"]),
    "--xcpc-border": safeToken("border", fb["--xcpc-border"]),
    "--xcpc-input": safeToken("input", fb["--xcpc-input"]),
    "--xcpc-primary": safeToken("primary", fb["--xcpc-primary"]),
    "--xcpc-primary-fg": safeToken("primary-foreground", fb["--xcpc-primary-fg"]),
    "--xcpc-accent": safeToken("accent", fb["--xcpc-accent"]),
    "--xcpc-destructive": safeToken("destructive", fb["--xcpc-destructive"]),
    "--xcpc-success": safeToken("chart-2", fb["--xcpc-success"]),
    "--xcpc-warning": safeToken("chart-4", fb["--xcpc-warning"]),
    "--xcpc-font-sans": UI_SANS,
    "--xcpc-font-mono": MONO,
  };
}

/** Generates a per-mount random bridge token so untrusted pages cannot forge host RPCs. */
function generateBridgeToken(): string {
  const bytes: string[] = [];
  for (let i = 0; i < 4; i++) {
    bytes.push(Math.floor(Math.random() * 0xffffffff).toString(16).padStart(8, "0"));
  }
  return bytes.join("");
}

function escapeInlineScriptJson(value: unknown): string {
  const json = JSON.stringify(value === undefined ? null : value);
  return json
    .replace(/<\/(script)/gi, "<\\/$1")
    .replace(/<!--/g, "<\\!--");
}

function formatCssVarsBlock(vars: Record<string, string>): string {
  return Object.entries(vars)
    .map(([k, v]) => `${k}:${v};`)
    .join("");
}

function buildIframeBridgeScript(opts: {
  pluginId: string;
  theme: "light" | "dark";
  cssVars: Record<string, string>;
  initialPayload?: unknown;
}): string {
  const bootstrapJson = escapeInlineScriptJson({
    pluginId: opts.pluginId,
    theme: opts.theme,
    cssVars: opts.cssVars,
    initialPayload: opts.initialPayload === undefined ? null : opts.initialPayload,
    hasInitialPayload: opts.initialPayload !== undefined,
  });

  return `(function(){
var boot=${bootstrapJson};
var currentTheme=boot.theme==="dark"?"dark":"light";
var currentVars=boot.cssVars||{};
var listeners=[];
var pending=Object.create(null);
var nextId=1;

function themeObj(){
  return {scheme:currentTheme,tokens:currentVars};
}

function applyTheme(theme,vars){
  currentTheme=theme==="dark"?"dark":"light";
  if(vars&&typeof vars==="object"){
    currentVars=vars;
  }
  try{
    var root=document.documentElement;
    if(root){
      root.setAttribute("data-theme",currentTheme);
      if(vars&&typeof vars==="object"){
        for(var k in vars){
          if(Object.prototype.hasOwnProperty.call(vars,k)){
            root.style.setProperty(k,String(vars[k]));
          }
        }
      }
    }
  }catch(_e){}
  var snapshot=themeObj();
  for(var i=0;i<listeners.length;i++){
    try{listeners[i](snapshot);}catch(_e){}
  }
}

applyTheme(currentTheme,boot.cssVars);

function postToParent(obj){
  try{
    if(window.parent&&window.parent!==window){
      window.parent.postMessage(obj,"*");
      return true;
    }
  }catch(_e){}
  return false;
}

function rpc(method,params){
  return new Promise(function(resolve,reject){
    var id=nextId++;
    pending[id]={resolve:resolve,reject:reject};
    if(!postToParent({__xcpc:"rpc",id:id,method:method,params:params||{}})){
      delete pending[id];
      reject(new Error("XCPC host bridge unavailable"));
    }
  });
}

function notify(method,params){
  postToParent({__xcpc:"notify",method:method,params:params||{}});
}

window.addEventListener("error",function(ev){
  var msg=(ev&&ev.message)||"Uncaught plugin error";
  var loc=ev&&ev.lineno?(" ("+ev.lineno+":"+(ev.colno||0)+")"):"";
  var stack=ev&&ev.error&&ev.error.stack?String(ev.error.stack):"";
  notify("runtimeError",{message:String(msg)+loc,details:stack});
});

window.addEventListener("unhandledrejection",function(ev){
  var reason=ev&&ev.reason;
  var msg=reason&&reason.message?String(reason.message):String(reason||"Unhandled promise rejection");
  var stack=reason&&reason.stack?String(reason.stack):"";
  notify("runtimeError",{message:msg,details:stack});
});

window.addEventListener("message",function(ev){
  if(ev.source!==window.parent)return;
  var data=ev&&ev.data;
  if(typeof data==="string"){
    try{data=JSON.parse(data);}catch(_e){return;}
  }
  if(!data||typeof data!=="object")return;
  if(data.__xcpc==="theme"){
    applyTheme(data.theme,data.cssVars);
    return;
  }
  if(data.__xcpc==="res"){
    var entry=pending[data.id];
    if(!entry)return;
    delete pending[data.id];
    if(data.ok)entry.resolve(data.result);
    else entry.reject(new Error(data.error||"Host call failed"));
  }
});

var xcpc={
  pluginId:boot.pluginId,
  initialPayload:boot.hasInitialPayload?boot.initialPayload:undefined,
  get theme(){return themeObj();},
  onThemeChange:function(cb){
    if(typeof cb!=="function")return function(){};
    listeners.push(cb);
    return function(){
      var idx=listeners.indexOf(cb);
      if(idx>=0)listeners.splice(idx,1);
    };
  },
  runTool:function(action,input){
    return rpc("runTool",{action:String(action||""),input:input===undefined?{}:input});
  },
  storage:{
    get:function(key){return rpc("storage.get",{key:String(key||"")});},
    set:function(key,value){return rpc("storage.set",{key:String(key||""),value:value});},
    remove:function(key){return rpc("storage.remove",{key:String(key||"")});},
    delete:function(key){return rpc("storage.remove",{key:String(key||"")});}
  },
  workspace:{
    getProblem:function(id){return rpc("workspace.getProblem",{id:String(id||"")});},
    listProblems:function(){return rpc("workspace.listProblems",{});},
    getContest:function(id){return rpc("workspace.getContest",{id:String(id||"")});},
    listContests:function(){return rpc("workspace.listContests",{});}
  },
  toast:function(text,tone){
    notify("toast",{text:String(text||""),tone:tone||"info"});
  },
  clipboard:{
    writeText:function(text){return rpc("clipboard.writeText",{text:String(text||"")});},
    writeImage:function(pngBase64){return rpc("clipboard.writeImage",{pngBase64:String(pngBase64||"")});}
  },
  files:{
    saveText:function(defaultName,content){
      return rpc("files.saveText",{defaultName:String(defaultName||"output.txt"),content:String(content||"")});
    },
    savePng:function(defaultName,pngBase64){
      return rpc("files.savePng",{defaultName:String(defaultName||"output.png"),pngBase64:String(pngBase64||"")});
    },
    pickFile:function(options){
      return rpc("files.pickFile",options||{});
    }
  },
  openTool:function(pluginId,toolIdOrPayload,maybePayload){
    if(typeof toolIdOrPayload==="string"){
      notify("openTool",{pluginId:String(pluginId||""),toolId:toolIdOrPayload,payload:maybePayload});
    }else{
      notify("openTool",{pluginId:String(pluginId||""),payload:toolIdOrPayload});
    }
  }
};

try{
  Object.defineProperty(window,"xcpc",{value:xcpc,configurable:false,writable:false});
}catch(_e){
  window.xcpc=xcpc;
}
})();`;
}

/**
 * Wraps raw plugin HTML inside a host-generated outer document with a strict CSP
 * and a sandboxed `<iframe sandbox="allow-scripts" srcdoc="...">`.
 *
 * Security invariants:
 * - Outer document enforces CSP before byte 0 of plugin HTML is parsed (immune to
 *   commented `<head>` or pre-`<head>` `<script>` bypasses).
 * - `sandbox="allow-scripts"` omits `allow-top-navigation` and `allow-same-origin`,
 *   preventing top-level navigation and direct access to `window.nsh.postMessage`.
 * - Outer shell authenticates relayed RPCs with a per-mount `bridgeToken` never
 *   exposed to the plugin iframe.
 */
export function injectPluginHtml(
  rawHtml: string,
  opts: {
    pluginId: string;
    theme: "light" | "dark";
    cssVars: Record<string, string>;
    initialPayload?: unknown;
    bridgeToken?: string;
  },
): string {
  const token = opts.bridgeToken ?? generateBridgeToken();
  const innerPrefix = [
    `<meta charset="UTF-8">`,
    `<meta http-equiv="Content-Security-Policy" content="${PLUGIN_WEBVIEW_CSP}">`,
    `<style id="xcpc-host-theme">:root{${formatCssVarsBlock(opts.cssVars)}}</style>`,
    `<script>${buildIframeBridgeScript(opts)}</script>`,
  ].join("\n");

  // Prepend cleanly after <!doctype> if present, never relying on regex-matching <head>.
  const doctypeMatch = /^\s*<!doctype\b[^>]*>/i.exec(rawHtml);
  const innerHtml = doctypeMatch
    ? `${doctypeMatch[0]}\n${innerPrefix}\n${rawHtml.slice(doctypeMatch[0].length)}`
    : `<!doctype html>\n${innerPrefix}\n${rawHtml}`;

  const outerBootstrap = escapeInlineScriptJson({
    token,
    srcdoc: innerHtml,
  });

  return `<!doctype html>
<html>
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="${PLUGIN_WEBVIEW_CSP}">
<style>html,body,iframe{margin:0;padding:0;width:100%;height:100%;border:0;display:block;overflow:hidden;background:transparent;}</style>
</head>
<body>
<iframe id="xcpc-plugin-frame" sandbox="allow-scripts"></iframe>
<script>
(function(){
var cfg=${outerBootstrap};
var frame=document.getElementById("xcpc-plugin-frame");
if(frame){frame.srcdoc=cfg.srcdoc;}
window.addEventListener("message",function(ev){
  if(frame&&ev.source===frame.contentWindow){
    if(ev.origin!=="null")return;
    var data=ev.data;
    if(!data||typeof data!=="object")return;
    if(data.__xcpc!=="rpc"&&data.__xcpc!=="notify")return;
    var wrapped={
      __xcpc:data.__xcpc,
      token:cfg.token,
      id:data.id,
      method:data.method,
      params:data.params
    };
    if(window.nsh&&typeof window.nsh.postMessage==="function"){
      window.nsh.postMessage(JSON.stringify(wrapped));
    }
    return;
  }
  var hostMsg=ev&&ev.data;
  if(typeof hostMsg==="string"){
    try{hostMsg=JSON.parse(hostMsg);}catch(_e){return;}
  }
  if(!hostMsg||typeof hostMsg!=="object")return;
  if((hostMsg.__xcpc==="res"||hostMsg.__xcpc==="theme")&&frame&&frame.contentWindow){
    frame.contentWindow.postMessage(hostMsg,"*");
  }
});
})();
</script>
</body>
</html>`;
}

interface BridgeRpcMessage {
  __xcpc: "rpc" | "notify";
  token?: string;
  id?: number;
  method: string;
  params?: Record<string, unknown>;
}

export function PluginWebView(props: {
  pluginId: string;
  getInitialPayload?: () => unknown;
}) {
  const htmlDoc = signal<string | null>(null);
  const errorInfo = signal<PluginUiErrorInfo | null>(null);
  const showDetails = signal(false);
  const reloadTick = signal(0);
  let webViewRef: WebViewRef | undefined;
  let loadGen = 0;
  let activeBridgeToken = "";
  let navigatedAway = false;

  const isTrustedPluginUrl = (url?: string): boolean => {
    if (!url) return true;
    const lower = url.trim().toLowerCase();
    return lower === "" || lower === "about:blank" || lower === "about:srcdoc";
  };

  const postToWebView = (payload: unknown) => {
    if (navigatedAway) return;
    try {
      webViewRef?.postMessage(JSON.stringify(payload));
    } catch {
      // Ignore if WebView is torn down mid-flight.
    }
  };

  // Load raw HTML whenever `pluginId`, per-plugin revision, or manual `reloadTick` changes.
  // Theme is read untracked here so light/dark switches update live via postMessage without reloading!
  effect(() => {
    const id = props.pluginId;
    void pluginRevisionFor(id);
    void reloadTick();

    loadGen += 1;
    navigatedAway = false;
    activeBridgeToken = generateBridgeToken();
    untracked(() => {
      showDetails.set(false);
      if (typeof preview.loadPluginUiHtml !== "function") {
        htmlDoc.set(null);
        errorInfo.set(classifyPluginUiError("loadPluginUiHtml bridge unavailable", "mount"));
        return;
      }

      try {
        const res = preview.loadPluginUiHtml(id);
        if (!res.ok || !res.html) {
          htmlDoc.set(null);
          const rawErr = res.errorCode
            ? `${res.errorCode}: ${res.error ?? ""}`
            : res.error ?? "Failed to load plugin UI";
          errorInfo.set(classifyPluginUiError(rawErr, "mount"));
          return;
        }
        const scheme: "light" | "dark" = colorScheme() === "dark" ? "dark" : "light";
        const injected = injectPluginHtml(res.html, {
          pluginId: id,
          theme: scheme,
          cssVars: buildThemeCssVars(scheme),
          initialPayload: props.getInitialPayload?.(),
          bridgeToken: activeBridgeToken,
        });
        errorInfo.set(null);
        htmlDoc.set(injected);
      } catch (err) {
        htmlDoc.set(null);
        errorInfo.set(
          classifyPluginUiError(err instanceof Error ? err.message : String(err), "mount"),
        );
      }
    });
  });

  // Live theme synchronization without reloading the WebView.
  effect(() => {
    const scheme: "light" | "dark" = colorScheme() === "dark" ? "dark" : "light";
    if (!htmlDoc()) return;
    postToWebView({
      __xcpc: "theme",
      theme: scheme,
      cssVars: buildThemeCssVars(scheme),
    });
  });

  cleanup(() => {
    loadGen += 1;
    webViewRef = undefined;
  });

  const executeRpc = async (method: string, params: Record<string, unknown>): Promise<unknown> => {
    const pluginId = props.pluginId;

    switch (method) {
      case "runTool": {
        const action = String(params.action ?? "");
        const inputJson = JSON.stringify(params.input ?? {});
        const res = await preview.pluginRunTool(pluginId, action, inputJson);
        if (!res.ok) {
          throw new Error(formatPluginError(res.error));
        }
        return res.reportJson ? JSON.parse(res.reportJson) : null;
      }

      case "storage.get": {
        const res = preview.pluginKvGet(pluginId, String(params.key ?? ""));
        if (!res.ok) throw new Error(res.error ?? "storage.get failed");
        if (!res.found || res.valueJson == null) return null;
        try {
          return JSON.parse(res.valueJson);
        } catch {
          return res.valueJson;
        }
      }

      case "storage.set": {
        const valueJson = JSON.stringify(params.value === undefined ? null : params.value);
        const res = preview.pluginKvSet(pluginId, String(params.key ?? ""), valueJson);
        if (!res.ok) throw new Error(res.error ?? "storage.set failed");
        return undefined;
      }

      case "storage.remove": {
        const res = preview.pluginKvRemove(pluginId, String(params.key ?? ""));
        if (!res.ok) throw new Error(res.error ?? "storage.remove failed");
        return undefined;
      }

      case "workspace.getProblem":
      case "workspace.listProblems":
      case "workspace.getContest":
      case "workspace.listContests": {
        const wsMethod = method.slice("workspace.".length);
        const res = preview.pluginWorkspaceQuery(
          pluginId,
          wsMethod,
          JSON.stringify(params ?? {}),
        );
        if (!res.ok) throw new Error(formatPluginError(res.error));
        return res.dataJson ? JSON.parse(res.dataJson) : null;
      }

      case "clipboard.writeText": {
        await Clipboard.writeText(String(params.text ?? ""));
        return undefined;
      }

      case "clipboard.writeImage": {
        const src = normalizeClipboardImageSrc(String(params.pngBase64 ?? ""));
        await Clipboard.writeImage(src);
        return undefined;
      }

      case "files.saveText": {
        const content = String(params.content ?? params.text ?? "");
        const defaultName = String(params.defaultName ?? "output.txt");
        const res = await Preview.pluginSaveTextDialog(content, defaultName, null);
        if (res.cancelled) return null;
        if (!res.ok) throw new Error(res.error ?? "Failed to save file");
        return res.path ?? defaultName;
      }

      case "files.savePng": {
        const pngBase64 = String(params.pngBase64 ?? "");
        const defaultName = String(params.defaultName ?? "output.png");
        const res = await Preview.pluginSavePngDialog(pngBase64, defaultName, null);
        if (res.cancelled) return null;
        if (!res.ok) throw new Error(res.error ?? "Failed to save PNG");
        return res.path ?? defaultName;
      }

      case "files.pickFile": {
        const exts = Array.isArray(params.extensions) ? params.extensions.map(String) : null;
        const title = params.title ? String(params.title) : null;
        const res = await Preview.pluginPickFileDialog(exts, title);
        if (res.cancelled) return null;
        if (!res.ok || !res.contentBase64) {
          throw new Error(res.error ?? "Failed to read file");
        }
        return {
          name: res.name ?? "file",
          content: res.text ?? "",
          base64: res.contentBase64,
        };
      }

      default:
        throw new Error(`Unknown xcpc bridge method: ${method}`);
    }
  };

  const handleMessage = (e: { data: string }) => {
    if (navigatedAway || !activeBridgeToken) return;
    let msg: BridgeRpcMessage;
    try {
      msg = JSON.parse(e.data) as BridgeRpcMessage;
    } catch {
      return;
    }
    if (!msg || typeof msg !== "object") return;
    // Reject any message not authenticated by our outer host shell's per-mount token.
    if (msg.token !== activeBridgeToken) return;
    const params = msg.params ?? {};

    if (msg.__xcpc === "notify") {
      if (msg.method === "toast") {
        const rawTone = String(params.tone ?? "info");
        const tone: ToastTone =
          rawTone === "issues" || rawTone === "error"
            ? "issues"
            : rawTone === "success"
              ? "success"
              : "info";
        showToast({ tone, text: String(params.text ?? "") });
      } else if (msg.method === "openTool") {
        const targetId = String(params.pluginId ?? "");
        const toolId = typeof params.toolId === "string" ? params.toolId : undefined;
        if (targetId) openToolPlugin(targetId, { toolId, payload: params.payload });
      } else if (msg.method === "runtimeError") {
        const errMsg = String(params.message ?? "Uncaught plugin error");
        const details = params.details ? String(params.details) : errMsg;
        errorInfo.set({
          title: zhEn("插件页面运行出错", "Plugin page runtime error"),
          summary: errMsg,
          details,
        });
      }
      return;
    }

    if (msg.__xcpc !== "rpc" || typeof msg.id !== "number") return;
    const reqId = msg.id;
    const gen = loadGen;

    void (async () => {
      try {
        const result = await executeRpc(msg.method, params);
        if (gen !== loadGen) return;
        postToWebView({ __xcpc: "res", id: reqId, ok: true, result });
      } catch (err) {
        if (gen !== loadGen) return;
        postToWebView({
          __xcpc: "res",
          id: reqId,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    })();
  };

  const handleWebViewError = (e: { message: string; code?: string }) => {
    if (e.code === "ERR_NSH_WEBVIEW_TERMINATED") {
      errorInfo.set({
        title: zhEn("插件页面进程已意外终止", "Plugin web process terminated"),
        summary: zhEn(
          "该插件的渲染进程已停止响应，点击下方按钮可重新加载。",
          "The plugin's web process exited unexpectedly. Reload to continue.",
        ),
        details: e.message || e.code,
      });
      return;
    }
    errorInfo.set({
      title: zhEn("插件页面加载失败", "Failed to load plugin page"),
      summary: formatPluginError(e.message, zhEn("无法渲染插件页面", "Unable to render plugin page")),
      details: e.code ? `${e.code}: ${e.message}` : e.message || undefined,
    });
  };

  const handleReload = () => {
    errorInfo.set(null);
    showDetails.set(false);
    reloadTick.set(reloadTick() + 1);
  };

  return (
    <View testID={`plugin-webview-host-${props.pluginId}`} style={{ flex: 1, minHeight: 0, flexDirection: "column" }}>
      <If
        when={errorInfo()}
        else={
          <If when={htmlDoc()}>
            {(doc) => (
              <WebView
                testID={`plugin-webview-${props.pluginId}`}
                ref={(node) => {
                  webViewRef = node;
                }}
                html={doc()}
                incognito
                onMessage={handleMessage}
                onLoadStart={(nav) => {
                  if (!isTrustedPluginUrl(nav?.url)) {
                    navigatedAway = true;
                    activeBridgeToken = "";
                  }
                }}
                onNavigationStateChange={(nav) => {
                  if (!isTrustedPluginUrl(nav?.url)) {
                    navigatedAway = true;
                    activeBridgeToken = "";
                  }
                }}
                onError={handleWebViewError}
                style={{ flex: 1 }}
              />
            )}
          </If>
        }
      >
        {(err) => (
          <View
            testID="plugin-webview-error"
            style={{
              flex: 1,
              alignItems: "center",
              justifyContent: "center",
              padding: 32,
              gap: 12,
            }}
          >
            <Icon
              name={err().untrusted ? "lock" : "triangleAlert"}
              size={24}
              tone={err().untrusted ? "muted-foreground" : "destructive"}
            />
            <Text style={{ fontSize: 14.5, fontWeight: 600 }} class="text-foreground">
              {err().title}
            </Text>
            <Text
              style={{ fontSize: 12.5, lineHeight: 18, textAlign: "center", maxWidth: 420 }}
              class="text-secondary-foreground"
            >
              {err().summary}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingTop: 2 }}>
              <If when={Boolean(err().untrusted)}>
                <Btn
                  testID="plugin-webview-open-settings"
                  variant="primary"
                  size="sm"
                  icon="settings"
                  label={zhEn("前往设置", "Open Settings")}
                  onPress={() => goSettings("plugins")}
                />
              </If>
              <Btn
                testID="plugin-webview-reload"
                variant={err().untrusted ? "outline" : "primary"}
                size="sm"
                icon="rotateCcw"
                label={zhEn("重新加载", "Reload")}
                onPress={handleReload}
              />
              <If when={Boolean(err().details)}>
                <Btn
                  testID="plugin-webview-toggle-details"
                  variant="ghost"
                  size="sm"
                  label={
                    showDetails()
                      ? zhEn("收起详情", "Hide details")
                      : zhEn("查看详情", "Show details")
                  }
                  onPress={() => showDetails.set(!showDetails())}
                />
              </If>
            </View>
            <If when={showDetails() && Boolean(err().details)}>
              <View
                class="bg-muted border border-border"
                style={{
                  maxWidth: 520,
                  width: "100%",
                  borderRadius: 8,
                  padding: 10,
                }}
              >
                <Text
                  style={{ fontSize: 11.5, lineHeight: 16, fontFamily: MONO }}
                  class="text-secondary-foreground"
                >
                  {err().details}
                </Text>
              </View>
            </If>
          </View>
        )}
      </If>
    </View>
  );
}
