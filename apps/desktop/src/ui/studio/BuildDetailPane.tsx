// Focused detail subcomponents for the Authoring console:
// - InlineFileEditor: frameless conflict-checked single-file editor (Fix A-3, P0-3)
// - CellDiagnosticDrawer: right-side overlay drawer with 3-column Input / Expected / Actual diff (P1-7)

import { Clipboard, colorScheme, effect, keymap, Scroll, signal, Text, untracked, View } from "nsh";
import { Editor } from "nsh/base";
import type { TestCasePreviewJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, IconBtn, tip } from "../kit";
import { errorOf, preview, type UiError } from "../store";
import { editorFontSize, editorLineHeight, editorMono, MONO, popoverShadow } from "../theme";
import {
  cellVisualStyle,
  formatBytesCompact,
  formatMemoryKib,
  type MatrixCellData,
} from "./buildLogic";

/**
 * Frameless inline file editor with explicit target relative path, SHA-256 conflict detection
 * (Fix A-3), 256 KiB safety cap, and external-editor escape hatch.
 */
export function InlineFileEditor(props: {
  problemId: string;
  relPath: string;
  readOnly: boolean;
  refreshKey?: number;
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
  const savedFeedback = signal(false);
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
      // Avoid clobbering unsaved user edits on background refresh for the same file.
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
    savedFeedback.set(true);
    setTimeout(() => savedFeedback.set(false), 1600);
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
      {/* Target path & save bar (P0-3: always displays target relative path) */}
      <View
        style={{
          height: 36,
          paddingLeft: 14,
          paddingRight: 12,
          borderBottomWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
        }}
        class="border-border bg-background/40"
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flex: 1, minWidth: 0 }}>
          <Text
            testID="inline-editor-target-path"
            style={{ fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-secondary-foreground"
          >
            {props.relPath}
          </Text>
          <If when={dirty()}>
            <View style={{ width: 6, height: 6, borderRadius: 3, flexShrink: 0 }} class="bg-chart-4" />
          </If>
          <If when={savedFeedback() && !dirty()}>
            <Text style={{ fontSize: 12, flexShrink: 0 }} class="text-secondary-foreground">
              {t("build.quickEdit.saved")}
            </Text>
          </If>
        </View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
          <Btn
            label={t("build.quickEdit.openExternal")}
            icon="folderOpen"
            variant="ghost"
            onPress={() => props.onOpenExternal(props.relPath)}
          />
          <If when={!props.readOnly && !tooLarge()}>
            <Btn
              testID="inline-file-save-btn"
              label={t("build.quickEdit.save")}
              icon="check"
              variant="primary"
              disabled={!dirty()}
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
            paddingLeft: 14,
            paddingRight: 12,
            paddingTop: 8,
            paddingBottom: 8,
            borderBottomWidth: 1,
            borderLeftWidth: 3,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 10,
          }}
          class="border-b-border border-l-chart-4 bg-muted/40"
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
                fontFamily={editorMono()}
                fontSize={editorFontSize()}
                lineHeight={Math.round(editorFontSize() * editorLineHeight())}
                activeLine
                class="bg-card"
                classes={{ content: "pl-3 pr-4 pt-3 pb-6" }}
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
 * P1-7 Right-side overlay diagnostic drawer: positioned over the right edge of the results
 * table so opening/closing NEVER squeezes or reflows the table. Shows 3-column Input / Expected
 * Answer / Actual Output snippets, stderr, copyable single-step repro command, and in-place jumps.
 */
export function CellDiagnosticDrawer(props: {
  problemId: string;
  cell: MatrixCellData;
  isAnomaly: boolean;
  genCmd?: string | null;
  solutionRelPath?: string;
  onClose: () => void;
  onOpenSolution: (relPath: string) => void;
  onOpenTest: (testIndex: number) => void;
}) {
  const testPreview = signal<TestCasePreviewJs | undefined>(undefined);
  const copied = signal(false);

  effect(() => {
    const slug = props.problemId;
    const idx = props.cell.test_index;
    untracked(() => {
      testPreview.set(preview.inspectTestCase(slug, String(idx), 12, 4));
    });
  });

  const badge = () => cellVisualStyle(props.cell.verdict, props.isAnomaly, colorScheme() === "dark");

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

  return (
    <View
      testID="cell-diagnostic-drawer"
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        bottom: 0,
        width: 460,
        borderLeftWidth: 1,
        boxShadow: popoverShadow(),
      }}
      class="bg-card border-border"
    >
      {/* Drawer Header */}
      <View
        style={{
          paddingLeft: 14,
          paddingRight: 10,
          paddingTop: 10,
          paddingBottom: 10,
          borderBottomWidth: 1,
          gap: 8,
        }}
        class="border-border"
      >
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flex: 1, minWidth: 0 }}>
            <View
              style={{
                height: 22,
                paddingLeft: 7,
                paddingRight: 7,
                borderRadius: 5,
                borderWidth: 1,
                backgroundColor: badge().bg,
                borderColor: badge().border,
                flexDirection: "row",
                alignItems: "center",
                gap: 4,
              }}
            >
              <If when={props.isAnomaly}>
                <Icon name="triangleAlert" size={11} tone="destructive" />
              </If>
              <Text style={{ fontSize: 11, fontWeight: 600, color: badge().fg, fontVariant: ["tabular-nums"] }}>
                {props.cell.verdict}
              </Text>
            </View>
            <Text
              style={{ fontSize: 13, fontWeight: 600, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
              class="text-foreground"
            >
              {`#${props.cell.test_index} ${props.cell.test_name}`}
            </Text>
          </View>
          <IconBtn icon="x" size={26} iconSize={14} onPress={props.onClose} />
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {`${props.cell.solution}   ${props.cell.cpu_time_ms} ms   ${formatMemoryKib(props.cell.peak_memory_kib)}`}
          </Text>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <If when={props.solutionRelPath}>
              {(rel) => (
                <Btn
                  testID="diag-open-solution-btn"
                  label={t("build.diag.openSolution")}
                  icon="code"
                  variant="ghost"
                  onPress={() => props.onOpenSolution(rel())}
                />
              )}
            </If>
            <Btn
              testID="diag-open-test-btn"
              label={t("build.diag.editTest")}
              icon="database"
              variant="ghost"
              onPress={() => props.onOpenTest(props.cell.test_index)}
            />
          </View>
        </View>
      </View>

      {/* Drawer Scroll Body */}
      <Scroll style={{ flex: 1 }} contentStyle={{ padding: 14, gap: 14 }}>
        {/* Checker message */}
        <If when={!!props.cell.checker_message}>
          <View
            style={{
              paddingLeft: 10,
              paddingTop: 6,
              paddingBottom: 6,
              borderLeftWidth: 2,
              gap: 3,
            }}
            class={props.isAnomaly ? "border-l-destructive" : "border-l-border"}
          >
            <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
              {t("build.diag.checkerOutput")}
            </Text>
            <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-foreground">
              {props.cell.checker_message}
            </Text>
          </View>
        </If>

        {/* 3-column Input / Expected Answer / Actual Output */}
        <View style={{ gap: 6 }}>
          <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
            {t("build.diag.ioComparison")}
          </Text>
          <View style={{ flexDirection: "row", gap: 8 }}>
            <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
              <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-secondary-foreground">
                {t("build.diag.input")}
              </Text>
              <Scroll style={{ borderRadius: 6, maxHeight: 168 }} contentStyle={{ padding: 8 }} class="bg-muted/60">
                <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                  {inputSnippet()}
                </Text>
              </Scroll>
            </View>
            <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
              <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-secondary-foreground">
                {t("build.diag.expected")}
              </Text>
              <Scroll style={{ borderRadius: 6, maxHeight: 168 }} contentStyle={{ padding: 8 }} class="bg-muted/60">
                <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                  {expectedSnippet()}
                </Text>
              </Scroll>
            </View>
            <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
              <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-secondary-foreground">
                {t("build.diag.actual")}
              </Text>
              <Scroll style={{ borderRadius: 6, maxHeight: 168 }} contentStyle={{ padding: 8 }} class="bg-muted/60">
                <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                  {actualSnippet()}
                </Text>
              </Scroll>
            </View>
          </View>
        </View>

        {/* Generator command */}
        <If when={!!props.genCmd}>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
              {t("build.diag.genCmd")}
            </Text>
            <View style={{ padding: 8, borderRadius: 6 }} class="bg-muted/60">
              <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-foreground">
                {props.genCmd!}
              </Text>
            </View>
          </View>
        </If>

        {/* Stderr */}
        <If when={!!props.cell.stderr}>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
              {t("build.diag.stderr")}
            </Text>
            <Scroll style={{ borderRadius: 6, maxHeight: 120 }} contentStyle={{ padding: 8 }} class="bg-muted/60">
              <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                {props.cell.stderr}
              </Text>
            </Scroll>
          </View>
        </If>

        {/* Copyable single-step repro command */}
        <If when={!!props.cell.repro_cmd}>
          <View style={{ gap: 4 }}>
            <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
              <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                {t("build.diag.reproCmd")}
              </Text>
              <Btn
                testID="diag-copy-repro-btn"
                label={copied() ? t("common.copied") : t("common.copy")}
                icon={copied() ? "check" : "copy"}
                variant="ghost"
                onPress={() => {
                  if (!props.cell.repro_cmd) return;
                  void Clipboard.writeText(props.cell.repro_cmd);
                  copied.set(true);
                  setTimeout(() => copied.set(false), 1600);
                }}
              />
            </View>
            <View style={{ padding: 8, borderRadius: 6 }} class="bg-muted/60">
              <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                {props.cell.repro_cmd!}
              </Text>
            </View>
          </View>
        </If>
      </Scroll>
    </View>
  );
}
