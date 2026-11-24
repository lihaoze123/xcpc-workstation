// Focused detail subcomponents for the Authoring console:
// - InlineFileEditor: unified frameless conflict-checked editor (P1-4, Fix A-3, P0-3, P2 no-ligatures)
// - CellDiagnosticDrawer: conclusion-first right-side overlay drawer with 3-column Input / Expected / Actual diff (P1-3, P1-7)

import { Clipboard, colorScheme, effect, keymap, Scroll, signal, Text, untracked, View, windowSize, type JSX } from "nsh";
import { Editor } from "nsh/base";
import type { TestCasePreviewJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon, type IconName } from "../Icon";
import { locale, t } from "../i18n";
import { Btn, createTransientFlag, IconBtn, tip } from "../kit";
import { errorOf, preview, type UiError } from "../store";
import { editorFontSize, editorLineHeight, editorMono, MONO, popoverShadow } from "../theme";
import {
  cellTimeMs,
  formatBytesCompact,
  formatMemoryKib,
  formatScoreRatioPercent,
  type MatrixCellData,
  type TranscriptEntryData,
} from "./buildLogic";
import { cellHeadline, groupTranscriptByRun } from "./statusLogic";

/** No-ligature monospace font stack so `!=`, `<=`, `>=` never render as ligatures in code/TOML editors (P2). */
function codeNoLigatureFont(): string {
  const base = editorMono();
  return base === "monospace" ? base : `'JetBrains Mono NL', ${base}`;
}

/**
 * Unified frameless inline file editor (P1-4):
 * - Single header (`Icon + Name + Role chip | Right actions`) with target relative path shown ONCE below the name
 * - Directly editable with `保存` button appearing cleanly only when dirty (never a faded disabled cyan CTA)
 * - SHA-256 external-edit conflict detection (Fix A-3), 256 KiB cap, 24px horizontal padding, no ligatures
 */
export function InlineFileEditor(props: {
  problemId: string;
  relPath: string;
  readOnly: boolean;
  refreshKey?: number;
  title?: string;
  icon?: IconName;
  badge?: string;
  extraActions?: JSX.Element;
  onSaved: () => void;
  onOpenExternal: (relPath: string) => void;
}) {
  const loadSeq = signal(0);
  const initialText = signal("");
  const baseHash = signal<string | undefined>(undefined);
  const sizeBytes = signal(0);
  const tooLarge = signal(false);
  const dirty = signal(false);
  const conflict = signal(false);
  const savedFeedback = createTransientFlag(1500);
  const err = signal<UiError | undefined>(undefined);
  let draftText = "";
  let loadedFor = "";

  function loadFile(slug: string, rel: string) {
    const res = preview.readPackageFile(slug, rel);
    loadedFor = `${slug}:${rel}`;
    if (!res.ok) {
      loadSeq.set(loadSeq() + 1);
      tooLarge.set(false);
      initialText.set("");
      baseHash.set(undefined);
      draftText = "";
      dirty.set(false);
      conflict.set(false);
      err.set({ code: res.code, error: res.error, context: t("ctx.openPackageFile") });
      return;
    }
    loadSeq.set(loadSeq() + 1);
    sizeBytes.set(res.sizeBytes);
    tooLarge.set(res.tooLarge);
    initialText.set(res.content);
    baseHash.set(res.contentHash || undefined);
    draftText = res.content;
    dirty.set(false);
    conflict.set(false);
    err.set(undefined);
  }

  effect(() => {
    const slug = props.problemId;
    const rel = props.relPath;
    void props.refreshKey;
    untracked(() => {
      if (!rel) return;
      if (loadedFor === `${slug}:${rel}` && dirty()) return;
      loadFile(slug, rel);
    });
  });

  function saveFile(forceOverwrite = false) {
    const slug = props.problemId;
    const rel = props.relPath;
    if (!rel || props.readOnly || untracked(tooLarge)) return;
    const expected = forceOverwrite ? null : (untracked(baseHash) ?? null);
    const res = preview.savePackageFile(slug, rel, draftText, expected);
    if (res.conflict) {
      conflict.set(true);
      err.set(undefined);
      return;
    }
    if (!res.ok) {
      err.set(errorOf(res, t("ctx.savePackageFile")));
      return;
    }
    err.set(undefined);
    conflict.set(false);
    dirty.set(false);
    initialText.set(draftText);
    if (res.contentHash) baseHash.set(res.contentHash);
    savedFeedback.trigger();
    props.onSaved();
  }

  keymap.bind(
    "mod+s",
    () => {
      if (!props.readOnly && untracked(dirty)) saveFile(false);
    },
    { when: "always" },
  );

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      {/* Unified Detail Header (P1-4): Name + Role chip + target relative path once below */}
      <View
        style={{
          minHeight: props.title ? 52 : 38,
          paddingLeft: 20,
          paddingRight: 16,
          paddingTop: 8,
          paddingBottom: 8,
          borderBottomWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
        }}
        class="border-border bg-background/30"
      >
        <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
          <If when={!!props.title}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 7 }}>
              <If when={!!props.icon}>
                <Icon name={props.icon!} size={14.5} tone="secondary-foreground" />
              </If>
              <Text
                style={{ fontSize: 13, fontWeight: 600, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }}
                class="text-foreground"
              >
                {props.title!}
              </Text>
              <If when={!!props.badge}>
                <View
                  style={{ height: 19, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center" }}
                  class="bg-muted"
                >
                  <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                    {props.badge!}
                  </Text>
                </View>
              </If>
              <If when={dirty()}>
                <View style={{ width: 6, height: 6, borderRadius: 3, flexShrink: 0 }} class="bg-chart-4" />
              </If>
              <If when={savedFeedback.active() && !dirty()}>
                <View style={{ flexDirection: "row", alignItems: "center", gap: 3, flexShrink: 0 }}>
                  <Icon name="check" size={11.5} tone="secondary-foreground" />
                  <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                    {t("build.quickEdit.saved")}
                  </Text>
                </View>
              </If>
            </View>
          </If>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text
              testID="inline-editor-target-path"
              style={{ fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
              class="text-muted-foreground"
            >
              {props.relPath}
            </Text>
            <If when={!props.title && dirty()}>
              <View style={{ width: 6, height: 6, borderRadius: 3, flexShrink: 0 }} class="bg-chart-4" />
            </If>
            <If when={!props.title && savedFeedback.active() && !dirty()}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 3, flexShrink: 0 }}>
                <Icon name="check" size={11.5} tone="secondary-foreground" />
                <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                  {t("build.quickEdit.saved")}
                </Text>
              </View>
            </If>
          </View>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
          {props.extraActions}
          <Btn
            label={t("build.quickEdit.openExternal")}
            icon="folderOpen"
            variant="ghost"
            onPress={() => props.onOpenExternal(props.relPath)}
          />
          <If when={!props.readOnly && !tooLarge() && dirty()}>
            <Btn
              testID="inline-file-save-btn"
              label={t("build.quickEdit.save")}
              icon="check"
              variant="primary"
              tooltip={tip(t("build.quickEdit.save"), "mod+s")}
              onPress={() => saveFile(false)}
            />
          </If>
        </View>
      </View>

      {/* Fix A-3: Conflict bar */}
      <If when={conflict()}>
        <View
          testID="inline-editor-conflict-bar"
          style={{
            paddingLeft: 20,
            paddingRight: 16,
            paddingTop: 8,
            paddingBottom: 8,
            borderBottomWidth: 1,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 10,
          }}
          class="border-b-border bg-muted/40"
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: 7, flex: 1, minWidth: 0 }}>
            <Icon name="triangleAlert" size={14} tone="chart-4" />
            <Text style={{ fontSize: 12, flex: 1 }} class="text-foreground">
              {t("build.quickEdit.conflictNotice")}
            </Text>
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
            <Btn
              label={t("build.quickEdit.loadExternal")}
              variant="outline"
              onPress={() => loadFile(props.problemId, props.relPath)}
            />
            <Btn
              label={t("build.quickEdit.overwriteSave")}
              variant="danger"
              onPress={() => saveFile(true)}
            />
          </View>
        </View>
      </If>

      <If when={err()}>
        <View style={{ padding: 12, borderBottomWidth: 1 }} class="border-border bg-muted/30">
          <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
        </View>
      </If>

      <If
        when={!tooLarge()}
        else={
          <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 28, gap: 12 }}>
            <Icon name="fileText" size={22} tone="muted-foreground" />
            <Text style={{ fontSize: 13, textAlign: "center" }} class="text-secondary-foreground">
              {`${t("build.quickEdit.tooLarge")} (${formatBytesCompact(sizeBytes())})`}
            </Text>
            <Btn
              label={t("build.quickEdit.openExternal")}
              icon="folderOpen"
              variant="primary"
              onPress={() => props.onOpenExternal(props.relPath)}
            />
          </View>
        }
      >
        <View style={{ flex: 1, minHeight: 0 }}>
          <Each of={[`${props.problemId}:${props.relPath}:${loadSeq()}`]} key={(k) => k}>
            {() => (
              <Editor
                style={{ flex: 1 }}
                defaultValue={initialText()}
                highlighting={false}
                folding={false}
                autoClose
                tabSize={2}
                hardTabs={false}
                softWrap={false}
                readOnly={props.readOnly}
                fontFamily={codeNoLigatureFont()}
                fontSize={editorFontSize()}
                lineHeight={Math.round(editorFontSize() * editorLineHeight())}
                activeLine
                class="bg-card"
                classes={{ content: "pl-6 pr-6 pt-4 pb-6" }}
                onChange={(e) => {
                  draftText = e.value;
                  dirty.set(e.value !== initialText());
                }}
              />
            )}
          </Each>
        </View>
      </If>
    </View>
  );
}

/**
 * P1-3 Conclusion-first right-side overlay diagnostic drawer:
 * - Line 1: Large human conclusion (`应为 WA，实际通过` amber / `应为 AC，实际 WA` red / `符合预期（WA）` muted)
 * - Order: Conclusion -> 3-column Input / Expected / Actual -> Single-step repro command -> Neutral checker output
 * - Responsive width so narrow windows are never crushed; absolute positioned so the table never reflows
 */
export function CellDiagnosticDrawer(props: {
  problemId: string;
  cell: MatrixCellData;
  expected: readonly string[];
  isAnomaly: boolean;
  genCmd?: string | null;
  solutionRelPath?: string;
  onClose: () => void;
  onOpenSolution: (relPath: string) => void;
  onOpenTest: (testIndex: number) => void;
}) {
  const testPreview = signal<TestCasePreviewJs | undefined>(undefined);
  const copied = createTransientFlag(1600);

  effect(() => {
    const slug = props.problemId;
    const idx = props.cell.test_index;
    untracked(() => {
      testPreview.set(preview.inspectTestCase(slug, String(idx), 12, 4));
    });
  });

  const headline = () => cellHeadline(props.cell.verdict, props.expected, props.isAnomaly);

  const drawerWidth = () => {
    const w = windowSize().width;
    if (w <= 0) return 460;
    return Math.min(460, Math.max(320, w - 280));
  };

  const inputSnippet = () => {
    const p = testPreview();
    if (!p?.ok || !p.availableOnDisk) return t("build.diag.unavailable");
    return p.head || t("build.tests.emptyContent");
  };

  const expectedSnippet = () => {
    const p = testPreview();
    if (!p?.ok) return t("build.diag.unavailable");
    if (p.stdOutputAvailable) return p.stdOutputHead || t("build.tests.emptyContent");
    if (p.ansAvailable) return p.ansHead || t("build.tests.emptyContent");
    return t("build.diag.unavailable");
  };

  const actualSnippet = () => {
    if (props.cell.output_preview && props.cell.output_preview.length > 0) {
      return props.cell.output_preview;
    }
    return t("build.diag.noOutputPreview");
  };

  const partialPct = () => formatScoreRatioPercent(props.cell.score_ratio, false);
  const transcript = () => props.cell.transcript ?? undefined;
  const transcriptGroups = () => {
    const tr = transcript();
    if (!tr) return { head: [], tail: [] };
    return { head: groupTranscriptByRun(tr.head ?? []), tail: groupTranscriptByRun(tr.tail ?? []) };
  };
  const multiRun = () => [...transcriptGroups().head, ...transcriptGroups().tail].some((g) => g.run != null && g.run > 1);

  /** 「判定说明」: the interactor's and the checker's words, merged unless both exist and differ (§5.4). */
  const verdictNotes = () => {
    const notes = [transcript()?.interactor_message, props.cell.checker_message]
      .map((n) => n?.trim())
      .filter((n): n is string => !!n);
    return notes.length === 2 && notes[0] === notes[1] ? [notes[0]] : notes;
  };

  /** 「测试 1　gen_n-3　std　2 ms　3.9 MB」: spaced, no `#`, wall time for TLE / ILE. */
  const meta = () =>
    [
      t("build.diag.meta", { index: props.cell.test_index }),
      props.cell.test_name,
      props.cell.solution,
      `${cellTimeMs(props.cell)} ms`,
      formatMemoryKib(props.cell.peak_memory_kib),
      partialPct() ? `${t("build.diag.partialScore")} ${partialPct()}` : undefined,
    ]
      .filter(Boolean)
      .join("　");

  const renderEntry = (entry: TranscriptEntryData) => {
    const toSolution = entry.direction === "interactor_to_solution";
    return (
      <View
        style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, paddingLeft: 8, borderLeftWidth: 2 }}
        class={toSolution ? "border-primary" : "border-chart-4"}
      >
        <Text style={{ fontSize: 12, fontWeight: 600, width: 12, flexShrink: 0 }} class={toSolution ? "text-primary" : "text-chart-4"}>
          {toSolution ? "→" : "←"}
        </Text>
        <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18, flex: 1 }} class="text-foreground">
          {entry.text}
        </Text>
      </View>
    );
  };

  const renderGroups = (groups: { run: number | null; entries: TranscriptEntryData[] }[]) =>
    groups.map((g) => (
      <View style={{ gap: 3 }}>
        <If when={multiRun() && g.run != null}>
          <Text style={{ fontSize: 12, paddingTop: 6 }} class="text-muted-foreground">
            {t("build.diag.run", { run: g.run ?? 1 })}
          </Text>
        </If>
        {g.entries.map((e) => renderEntry(e))}
      </View>
    ));

  const block = (label: string, body: JSX.Element) => (
    <View style={{ gap: 6 }}>
      <Text style={{ fontSize: 12 }} class="text-muted-foreground">
        {label}
      </Text>
      {body}
    </View>
  );

  const mono = (text: string, maxHeight?: number) => (
    <Scroll style={{ borderRadius: 6, maxHeight }} contentStyle={{ padding: 8 }} class="bg-muted">
      <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-foreground">
        {text}
      </Text>
    </Scroll>
  );

  return (
    <View
      testID="cell-diagnostic-drawer"
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        bottom: 0,
        width: drawerWidth(),
        borderLeftWidth: 1,
        boxShadow: popoverShadow(),
      }}
      class="bg-card border-border"
    >
      {/* Header: the verdict in words (or 「应 AC，实际 WA」), then where it happened */}
      <View style={{ paddingLeft: 16, paddingRight: 10, paddingTop: 12, paddingBottom: 12, borderBottomWidth: 1, gap: 6 }} class="border-border">
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <Text
            testID="diag-conclusion-headline"
            style={{ fontSize: 14, fontWeight: 600, flex: 1, numberOfLines: 2 }}
            class={headline().mismatch ? "text-destructive" : "text-foreground"}
          >
            {headline().text}
          </Text>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 2, flexShrink: 0 }}>
            <If when={props.solutionRelPath}>
              {(rel) => (
                <Btn testID="diag-open-solution-btn" label={t("build.diag.openSolution")} variant="ghost" onPress={() => props.onOpenSolution(rel())} />
              )}
            </If>
            <Btn testID="diag-open-test-btn" label={t("build.diag.editTest")} variant="ghost" onPress={() => props.onOpenTest(props.cell.test_index)} />
            <IconBtn icon="x" size={26} iconSize={14} onPress={props.onClose} />
          </View>
        </View>
        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }} class="text-secondary-foreground">
          {meta()}
        </Text>
      </View>

      <View style={{ flex: 1, minHeight: 0, padding: 16, gap: 14 }}>
        {/* Only a mismatch gets an explanation */}
        <If when={headline().explain}>
          {(ex) => (
            <View testID="diag-explain" style={{ padding: 10, borderRadius: 6 }} class="bg-destructive/12">
              <Text style={{ fontSize: 13, lineHeight: 19 }} class="text-foreground">
                {ex()}
              </Text>
            </View>
          )}
        </If>

        <If when={verdictNotes().length > 0}>
          {block(
            t("build.diag.verdictNote"),
            <View style={{ gap: 6 }}>{verdictNotes().map((n) => mono(n, 96))}</View>,
          )}
        </If>

        <If when={!!props.genCmd}>{block(t("build.diag.genCmd"), mono(props.genCmd ?? ""))}</If>

        {/* Interactive: the transcript fills the rest */}
        <If when={transcript()}>
          {(tr) => (
            <View testID="diag-interaction-transcript" style={{ flex: 1, minHeight: 120, gap: 6 }}>
              <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                  {t("build.diag.transcript")}
                </Text>
                <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                  {t("build.diag.transcriptLegend")}
                </Text>
              </View>
              <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ gap: 3, paddingBottom: 8 }}>
                {renderGroups(transcriptGroups().head)}
                <If when={tr().omitted_entries > 0}>
                  <Text style={{ fontSize: 12, paddingTop: 4, paddingBottom: 4, textAlign: "center", fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {t("build.diag.transcriptOmitted", { count: tr().omitted_entries })}
                  </Text>
                </If>
                {renderGroups(transcriptGroups().tail)}
              </Scroll>
            </View>
          )}
        </If>

        {/* Standard: input / answer / output, then how to reproduce */}
        <If when={!transcript()}>
          <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ gap: 14 }}>
            <View style={{ flexDirection: "row", gap: 8 }}>
              <View style={{ flex: 1, minWidth: 0 }}>{block(t("build.diag.input"), mono(inputSnippet(), 168))}</View>
              <View style={{ flex: 1, minWidth: 0 }}>{block(t("build.diag.expected"), mono(expectedSnippet(), 168))}</View>
              <View style={{ flex: 1, minWidth: 0 }}>{block(t("build.diag.actual"), mono(actualSnippet(), 168))}</View>
            </View>
          </Scroll>
        </If>

        <If when={!!props.cell.stderr}>{block(t("build.diag.stderr"), mono(props.cell.stderr ?? "", 120))}</If>

        <If when={!!props.cell.repro_cmd}>
          <View style={{ gap: 6 }}>
            <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                {t("build.diag.reproCmd")}
              </Text>
              <Btn
                testID="diag-copy-repro-btn"
                label={copied.active() ? t("common.copied") : t("common.copy")}
                icon={copied.active() ? "check" : "copy"}
                variant="ghost"
                style={{ minWidth: 74 }}
                onPress={() => {
                  if (!props.cell.repro_cmd) return;
                  void Clipboard.writeText(props.cell.repro_cmd);
                  copied.trigger();
                }}
              />
            </View>
            {mono(props.cell.repro_cmd ?? "")}
          </View>
        </If>
      </View>
    </View>
  );
}
