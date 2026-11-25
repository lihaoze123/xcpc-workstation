// Friendly localized error formatting for plugin install, repository fetch, and QuickJS UI sandbox errors.

import { zhEn } from "../training/locale";

export interface PluginUiErrorInfo {
  title: string;
  summary: string;
  details?: string;
  untrusted?: boolean;
}

/** Maps raw Core/CLI plugin error codes & transport errors into concise user-facing text. */
export function formatPluginError(raw?: string | null, fallback?: string): string {
  const msg = (raw ?? "").trim();
  if (!msg) return fallback ?? zhEn("操作失败", "Operation failed");

  const upper = msg.toUpperCase();
  const lower = msg.toLowerCase();

  if (upper.includes("CHECKSUM_MISMATCH") || lower.includes("sha256") && lower.includes("mismatch")) {
    return zhEn("安装包校验失败，可能已被篡改，已取消安装", "Package checksum mismatch; install cancelled for safety");
  }
  if (upper.includes("SYMLINK_ESCAPE") || lower.includes("symlink") && lower.includes("escape")) {
    return zhEn("插件包包含越界符号链接，已拒绝安装", "Plugin package contains an escaping symlink; install rejected");
  }
  if (upper.includes("ZIP_SLIP") || lower.includes("path traversal")) {
    return zhEn("插件包包含非法文件路径，已拒绝安装", "Plugin package contains an invalid file path; install rejected");
  }
  if (upper.includes("PLUGIN_UNTRUSTED")) {
    return zhEn(
      "该插件尚未确认信任，请先在「设置 → 插件」中确认信任",
      "This plugin is not trusted yet. Confirm trust in Settings → Plugins first",
    );
  }
  if (upper.includes("HTTP_NOT_ALLOWED")) {
    return zhEn(
      "仅允许添加 HTTPS 仓库地址（本地回环地址除外）",
      "Only HTTPS repository URLs are allowed (except localhost)",
    );
  }
  if (upper.includes("UNSUPPORTED_UI_ENTRY")) {
    return zhEn(
      "该插件使用旧版界面格式，请更新或重新安装最新版本的插件",
      "This plugin uses an outdated UI format. Please update or reinstall the latest version",
    );
  }
  if (upper.includes("INVALID_PLUGIN_ID") || upper.includes("INVALID_MANIFEST")) {
    return zhEn("插件清单格式无效，无法安装或加载", "Invalid plugin manifest");
  }
  if (upper.includes("PLUGIN_NOT_FOUND")) {
    return zhEn("未找到该插件", "Plugin not found");
  }
  const isNetworkError =
    lower.includes("dns error") ||
    lower.includes("dns failed") ||
    lower.includes("failed to lookup") ||
    lower.includes("connection refused") ||
    lower.includes("connection failed") ||
    lower.includes("connect error") ||
    lower.includes("error sending request") ||
    lower.includes("connection reset") ||
    lower.includes("failed to connect") ||
    msg.includes("连接被拒绝") ||
    msg.includes("无法连接") ||
    msg.includes("连接超时") ||
    msg.includes("名称解析失败") ||
    (upper.includes("HTTP_ERROR") && lower.includes("timed out"));

  if (isNetworkError) {
    return zhEn("网络连接失败，无法访问插件仓库", "Network error: unable to reach plugin repository");
  }
  if (
    upper.includes("TIMEOUT") ||
    lower.includes("timed out") ||
    lower.includes("interrupted") ||
    msg.includes("超时")
  ) {
    return zhEn("插件执行超时，已在沙箱中中止", "Plugin execution timed out and was stopped");
  }
  if (
    upper.includes("MEMORY_LIMIT") ||
    lower.includes("out of memory") ||
    lower.includes("memory limit") ||
    msg.includes("内存超限")
  ) {
    return zhEn("插件超出内存限制，已在沙箱中中止", "Plugin exceeded memory limit and was stopped");
  }
  if (
    upper.includes("JSON_ERROR") ||
    lower.includes("expected value at line") ||
    lower.includes("eof while parsing") ||
    lower.includes("invalid json")
  ) {
    return zhEn(
      "仓库索引格式无效，请检查地址是否为有效的 index.json",
      "Invalid repository index format (expected a valid index.json)",
    );
  }
  if (
    lower.includes("status 404") ||
    lower.includes("404 not found") ||
    lower.includes("http 404")
  ) {
    return zhEn("未找到远程文件（404），请稍后重试或检查仓库地址", "Remote file not found (404). Check the repository URL or retry later");
  }

  // Strip duplicate error code prefixes like "FOO_BAR: FOO_BAR: ..." and raw HTTP_ERROR: prefixes.
  const cleaned = msg
    .replace(/^([A-Z0-9_]+):\s*\1:\s*/u, "$1: ")
    .replace(/^HTTP_ERROR:\s*/iu, "");
  const firstLine = cleaned.split(/\r?\n/u)[0]?.trim() || cleaned;
  return firstLine;
}

/** Classifies a QuickJS sandbox UI error into a clear title, concise summary, and collapsible raw stack trace. */
export function classifyPluginUiError(
  rawError: string | null | undefined,
  phase: "mount" | "action",
): PluginUiErrorInfo {
  const raw = (rawError ?? "").trim();
  const upper = raw.toUpperCase();
  const lower = raw.toLowerCase();

  if (upper.includes("PLUGIN_UNTRUSTED")) {
    return {
      title: zhEn("插件尚未信任", "Plugin not trusted"),
      summary: zhEn(
        "该插件声明了敏感权限，请前往「设置 → 插件」确认信任后再使用。",
        "This plugin requests sensitive permissions. Confirm trust in Settings → Plugins before using it.",
      ),
      untrusted: true,
    };
  }

  const isTimeout =
    upper.includes("TIMEOUT") ||
    lower.includes("timed out") ||
    lower.includes("interrupted") ||
    raw.includes("超时");
  const isOom =
    upper.includes("MEMORY_LIMIT") ||
    lower.includes("out of memory") ||
    lower.includes("memory limit") ||
    raw.includes("内存超限");

  const hasStack = raw.includes("\n") || raw.includes("xcpc-plugin://") || raw.includes("    at ");
  const firstLine = raw.split(/\r?\n/u).find((l) => l.trim().length > 0)?.trim() ?? "";

  if (phase === "mount") {
    return {
      title: zhEn("插件界面加载失败", "Failed to load plugin UI"),
      summary: isTimeout
        ? zhEn("插件初始化超时，已在沙箱中自动中止。", "Plugin initialization timed out in the sandbox.")
        : isOom
          ? zhEn("插件初始化时超出内存限制，已在沙箱中自动中止。", "Plugin exceeded memory limit during initialization.")
          : firstLine
            ? formatPluginError(firstLine)
            : zhEn("插件界面初始化时发生异常，可尝试重新加载。", "An error occurred while initializing the plugin UI."),
      details: hasStack || (raw.length > 0 && raw !== firstLine) ? raw : raw || undefined,
    };
  }

  if (isTimeout) {
    return {
      title: zhEn("插件操作超时", "Plugin action timed out"),
      summary: zhEn(
        "本次操作响应超时，已在沙箱中自动中止。可点击下方按钮重新加载插件。",
        "This action timed out and was stopped in the sandbox. Reload the plugin to continue.",
      ),
      details: raw || undefined,
    };
  }

  if (isOom) {
    return {
      title: zhEn("插件超出内存限制", "Plugin exceeded memory limit"),
      summary: zhEn(
        "本次操作内存占用超限，已在沙箱中自动中止。可点击下方按钮重新加载插件。",
        "This action exceeded the memory limit and was stopped. Reload the plugin to continue.",
      ),
      details: raw || undefined,
    };
  }

  return {
    title: zhEn("插件操作执行失败", "Plugin action failed"),
    summary: firstLine
      ? formatPluginError(firstLine)
      : zhEn("插件在响应交互时发生异常，可尝试重新加载。", "The plugin encountered an error while handling the action."),
    details: raw || undefined,
  };
}
