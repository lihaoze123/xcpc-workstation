import type { AppUpdate, UpdateProgress } from "nsh/updater";
import { t } from "./i18n";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; update: AppUpdate }
  | { kind: "downloading"; update: AppUpdate; progress: UpdateProgress }
  | { kind: "ready"; update: AppUpdate }
  | { kind: "scheduled"; update: AppUpdate }
  | { kind: "error"; message: string };

/** Primary action button label when an update is available. */
export function updateAvailableActionLabel(update: Pick<AppUpdate, "canInstall">): string {
  return update.canInstall ? t("update.download") : t("update.openReleasePage");
}

/** Trimmed release notes to display for the current update state, or empty string when hidden. */
export function visibleUpdateNotes(state: UpdateState): string {
  if (
    state.kind === "available" ||
    state.kind === "downloading" ||
    state.kind === "ready" ||
    state.kind === "scheduled"
  ) {
    return state.update.notes?.trim() ?? "";
  }
  return "";
}

/** Subtitle shown under "检查更新" / "Software Update" for a given update state. */
export function formatUpdateSummary(
  state: UpdateState,
  devShell: boolean,
  formatVersion: (v: string) => string,
  formatBytes: (b: number) => string,
): string | undefined {
  if (devShell) return t("update.devBuild");
  switch (state.kind) {
    case "idle":
      return undefined;
    case "checking":
      return t("update.checking");
    case "upToDate":
      return t("update.upToDate");
    case "available":
      return t("update.available", { version: formatVersion(state.update.version) });
    case "downloading": {
      const { received, total } = state.progress;
      if (total && total > 0) {
        const pct = Math.min(100, Math.round((received / total) * 100));
        return t("update.downloadingWithTotal", {
          version: formatVersion(state.update.version),
          pct,
          received: formatBytes(received),
          total: formatBytes(total),
        });
      }
      return t("update.downloadingNoTotal", {
        version: formatVersion(state.update.version),
        received: formatBytes(received),
      });
    }
    case "ready":
      return t("update.ready", { version: formatVersion(state.update.version) });
    case "scheduled":
      return t("update.scheduled", { version: formatVersion(state.update.version) });
    case "error":
      return state.message;
  }
}

const DEFAULT_GITHUB_REPO = "lihaoze123/xcpc-workstation";
const GITHUB_DOWNLOAD_RE = /^https:\/\/github\.com\/([^/]+\/[^/]+)\/releases\/download\/([^/]+)\/.+$/;
const GITHUB_RELEASE_PAGE_RE = /^https:\/\/github\.com\/[^/]+\/[^/]+\/releases\/(?:tag\/[^/]+|latest)\/?$/;

/**
 * Resolve the human-facing GitHub Release page URL for a read-only installation
 * (`canInstall === false`), converting a direct asset download URL to its tag page.
 */
export function resolveReleasePageUrl(update: Pick<AppUpdate, "url" | "version">): string {
  const url = (update.url ?? "").trim();
  const dlMatch = GITHUB_DOWNLOAD_RE.exec(url);
  if (dlMatch) {
    return `https://github.com/${dlMatch[1]}/releases/tag/${dlMatch[2]}`;
  }
  if (GITHUB_RELEASE_PAGE_RE.test(url)) {
    return url;
  }
  const version = (update.version ?? "").trim().replace(/^v/i, "");
  if (version) {
    return `https://github.com/${DEFAULT_GITHUB_REPO}/releases/tag/v${version}`;
  }
  return url;
}

export type InlineSpan =
  | { kind: "text"; text: string }
  | { kind: "bold"; text: string }
  | { kind: "code"; text: string };

export type UpdateNoteBlock =
  | { kind: "heading"; text: string }
  | { kind: "bullet"; level: 0 | 1; spans: InlineSpan[] }
  | { kind: "paragraph"; spans: InlineSpan[] };

/** Parse lightweight inline Markdown (`**bold**` and `` `code` ``) into flat spans. */
export function parseInlineSpans(text: string): InlineSpan[] {
  const spans: InlineSpan[] = [];
  let buf = "";
  let i = 0;

  const flush = () => {
    if (buf) {
      spans.push({ kind: "text", text: buf });
      buf = "";
    }
  };

  while (i < text.length) {
    // Inline code: one or more backticks matched by the exact same count of backticks.
    if (text[i] === "`") {
      let tickCount = 0;
      while (i + tickCount < text.length && text[i + tickCount] === "`") {
        tickCount++;
      }
      const fence = "`".repeat(tickCount);
      const closeIdx = text.indexOf(fence, i + tickCount);
      if (closeIdx !== -1) {
        let code = text.slice(i + tickCount, closeIdx);
        if (code.length >= 2 && code.startsWith(" ") && code.endsWith(" ") && code.trim().length > 0) {
          code = code.slice(1, -1);
        }
        flush();
        spans.push({ kind: "code", text: code });
        i = closeIdx + tickCount;
        continue;
      }
      buf += fence;
      i += tickCount;
      continue;
    }

    // Bold: `**...**`
    if (text[i] === "*" && text[i + 1] === "*") {
      const closeIdx = text.indexOf("**", i + 2);
      if (closeIdx !== -1 && closeIdx > i + 2) {
        flush();
        spans.push({ kind: "bold", text: text.slice(i + 2, closeIdx) });
        i = closeIdx + 2;
        continue;
      }
      buf += "**";
      i += 2;
      continue;
    }

    buf += text[i];
    i++;
  }

  flush();
  return spans;
}

/**
 * Parse Keep a Changelog Markdown into structured blocks for native rendering:
 * - `### Heading`
 * - `- Item` / `  - Sub-item`
 * - Plain paragraphs
 */
export function parseUpdateNotes(markdown: string): UpdateNoteBlock[] {
  const blocks: UpdateNoteBlock[] = [];
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  let paragraphLines: string[] = [];

  const flushParagraph = () => {
    if (paragraphLines.length === 0) return;
    const joined = paragraphLines.join(" ").trim();
    paragraphLines = [];
    if (joined) {
      blocks.push({ kind: "paragraph", spans: parseInlineSpans(joined) });
    }
  };

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (!trimmed) {
      flushParagraph();
      continue;
    }

    const headingMatch = /^###\s+(.+)$/.exec(trimmed);
    if (headingMatch) {
      flushParagraph();
      blocks.push({ kind: "heading", text: headingMatch[1].trim() });
      continue;
    }

    const bulletMatch = /^([ \t]*)[-*]\s+(.+)$/.exec(rawLine);
    if (bulletMatch) {
      flushParagraph();
      const indent = bulletMatch[1].replace(/\t/g, "  ").length;
      const level: 0 | 1 = indent >= 2 ? 1 : 0;
      blocks.push({
        kind: "bullet",
        level,
        spans: parseInlineSpans(bulletMatch[2].trim()),
      });
      continue;
    }

    paragraphLines.push(trimmed);
  }

  flushParagraph();
  return blocks;
}
