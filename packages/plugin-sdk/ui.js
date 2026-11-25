// Virtual module stub: at build time `@xcpc/plugin-sdk/vite` rewrites imports of
// `@xcpc/plugin-sdk/ui` to read directly from the host-injected `sdk` instance.
function currentSdk() {
  const sdk = globalThis.__xcpc_current_sdk;
  if (!sdk) {
    throw new Error("@xcpc/plugin-sdk/ui must be executed inside XCPC Workstation render(sdk)");
  }
  return sdk;
}

export const getSdk = () => currentSdk();
export const h = (...args) => currentSdk().h(...args);
export const Fragment = (props) => props?.children ?? null;
export const signal = (...args) => currentSdk().signal(...args);
export const computed = (...args) => currentSdk().computed(...args);
export const effect = (...args) => currentSdk().effect(...args);
export const untracked = (...args) => currentSdk().untracked(...args);
export const token = (...args) => currentSdk().token(...args);
export const colorScheme = (...args) => currentSdk().colorScheme(...args);
export const runTool = (...args) => currentSdk().runTool(...args);
export const useTool = (...args) => currentSdk().useTool(...args);
export const onCleanup = (...args) => currentSdk().onCleanup(...args);
export const onShortcut = (...args) => currentSdk().onShortcut(...args);
export const openTool = (...args) => currentSdk().openTool(...args);
export const pickFile = (...args) => currentSdk().pickFile(...args);
export const readClipboard = (...args) => currentSdk().readClipboard(...args);
export const openExternal = (...args) => currentSdk().openExternal(...args);
export const renderSvgToPng = (...args) => currentSdk().renderSvgToPng(...args);
export const savePng = (...args) => currentSdk().savePng(...args);
export const saveText = (...args) => currentSdk().saveText(...args);
export const copyText = (...args) => currentSdk().copyText(...args);
export const toast = (...args) => currentSdk().toast(...args);
export const initialPayload = (...args) => currentSdk().initialPayload(...args);

export const ui = new Proxy(
  {},
  {
    get(_target, prop) {
      return currentSdk().ui[prop];
    },
  },
);

export const canvas = new Proxy(
  {},
  {
    get(_target, prop) {
      return currentSdk().canvas[prop];
    },
  },
);

export const storage = new Proxy(
  {},
  {
    get(_target, prop) {
      return currentSdk().storage[prop];
    },
  },
);

export const workspace = new Proxy(
  {},
  {
    get(_target, prop) {
      return currentSdk().workspace[prop];
    },
  },
);
