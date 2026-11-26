// Full-screen Single Snippet Studio: auto-saving left editor (`代码 | 说明 | 验板`),
// center live Typst preview (`PreviewCanvas` inheriting referencing TRD template & options),
// and right collapsible 320pt Info drawer (`Cmd+I`) for title/tags/complexity/verify specs.

import { cleanup, Clipboard, computed, effect, keymap, Scroll, signal, Text, View } from "nsh";
import { Editor, Input, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import type { PreviewRequest } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t, type MessageKey } from "../i18n";
import { Btn, Chip, createTransientFlag, DelayedSpinner, IconBtn, Segmented, tip } from "../kit";
import { SectionEmpty } from "../layout";
import { InsetPanel } from "../layout";
import { AppMenu, type MenuEntry } from "../Menu";
import { PreviewCanvas, type CanvasController } from "../studio/PreviewCanvas";
import { TitleBar } from "../TitleBar";
import { showToast } from "../Toast";
import {
  activeSnippetStudio,
  bumpSnippetsRevision,
  closeSnippetStudio,
  goSettings,
  info,
  preview,
  registerQuitFlusher,
  revision,
  snippetsRevision,
} from "../store";
import { cardShadow, editorFontSize, editorLineHeight, MONO, monoFont } from "../theme";
import {
  encodeStringPatch,
  parseJsonPayload,
  resolveSnippetPreviewTemplate,
  snippetStatusStyle,
} from "./snippetLogic";
import type { SnippetDetailPayload, SnippetVerifySpec, TrdDetailPayload, TrdSummaryItem } from "./types";
import { runSnippetVerifyJob } from "./verifyJob";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, ROW, SPACE } from "../tokens";
type SnippetTab = "code" | "notes" | "verify";
type VerifyKindOption = "self" | "library_checker" | "problem";

export function SnippetStudio(props: { active: boolean }) {
  // Retain last active slug while SpaceStage slides out symmetrically.
  const slug = computed<string>((prev) => activeSnippetStudio() ?? prev ?? "");
  const payload = signal<SnippetDetailPayload | undefined>(undefined);
  const previewTemplate = signal<string>("trd-compact");
  const tab = signal<SnippetTab>("code");
  const infoOpen = signal(false);
  const codeText = signal("");
  const notesText = signal("");
  const dirtyCode = signal(false);
  const dirtyNotes = signal(false);
  const verifying = signal(false);
  const showVerifySpinner = signal(false);

  // Editable metadata fields inside the collapsible Info drawer (review A)
  const metaTitle = signal("");
  const metaDesc = signal("");
  const metaTime = signal("");
  const metaSpace = signal("");
  const metaTags = signal("");
  const newVerifyKind = signal<VerifyKindOption>("self");
  const newVerifyTarget = signal("");
  const newVerifyAdapter = signal("");

  let verifySpinTimer: ReturnType<typeof setTimeout> | undefined;
  let autoSaveTimer: ReturnType<typeof setTimeout> | undefined;
  let compileTimer: ReturnType<typeof setTimeout> | undefined;
  let canvasCtrl: CanvasController | undefined;

  effect(() => {
    const busy = verifying();
    clearTimeout(verifySpinTimer);
    if (!busy) showVerifySpinner.set(false);
    else verifySpinTimer = setTimeout(() => showVerifySpinner.set(true), 150);
  });
  const copiedFlag = createTransientFlag(1500);

  function resolveTemplateFor(targetSlug: string) {
    const trds = parseJsonPayload<{ trds: TrdSummaryItem[] }>(preview.listTrdsJson())?.trds ?? [];
    const trdDetails: { id: string; template?: string | null; chapters?: { title: string; items: string[] }[] }[] = [];
    for (const trd of trds) {
      const det = parseJsonPayload<TrdDetailPayload>(preview.loadTrdJson(trd.id));
      if (det) {
        trdDetails.push({ id: trd.id, template: det.detail.meta.template, chapters: det.detail.meta.chapter ?? [] });
      }
    }
    previewTemplate.set(resolveSnippetPreviewTemplate(targetSlug, trdDetails, "trd-compact"));
  }

  function loadDetail(targetSlug: string, resetTab = false) {
    if (!targetSlug) return;
    const res = preview.loadSnippetJson(targetSlug);
    const parsed = parseJsonPayload<SnippetDetailPayload>(res);
    if (!parsed) return;
    payload.set(parsed);
    metaTitle.set(parsed.detail.meta.title ?? "");
    metaDesc.set(parsed.detail.meta.description ?? "");
    metaTime.set(parsed.detail.meta.time_complexity ?? "");
    metaSpace.set(parsed.detail.meta.space_complexity ?? "");
    metaTags.set((parsed.detail.meta.tags ?? []).join(", "));
    if (!dirtyCode()) codeText.set(parsed.detail.code?.text ?? "");
    if (!dirtyNotes()) notesText.set(parsed.detail.notes?.text ?? "");
    if (resetTab) {
      tab.set(parsed.detail.code ? "code" : "notes");
      resolveTemplateFor(targetSlug);
    }
  }

  function saveSnippetMeta(extra?: { verify?: SnippetVerifySpec[] }) {
    const s = slug();
    if (!s || info().readOnly) return;
    const tagList = metaTags()
      .split(/[,，]/)
      .map((x) => x.trim())
      .filter(Boolean);
    const patch: Record<string, unknown> = {
      title: metaTitle().trim() || payload()?.detail.meta.title || s,
      tags: tagList,
      time_complexity: encodeStringPatch(metaTime()),
      space_complexity: encodeStringPatch(metaSpace()),
      description: encodeStringPatch(metaDesc()),
    };
    if (extra?.verify !== undefined) {
      patch.verify = extra.verify;
    }
    const res = preview.updateSnippetMeta(s, JSON.stringify(patch));
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.updateFailed") });
      return;
    }
    bumpSnippetsRevision();
    loadDetail(s, false);
    canvasCtrl?.compile();
  }

  function addVerifySpec() {
    const kind = newVerifyKind();
    const target = newVerifyTarget().trim();
    const adapter = newVerifyAdapter().trim();
    if (!target) return;
    if (kind !== "self" && !adapter) return;
    const nextSpec: SnippetVerifySpec =
      kind === "self"
        ? { kind: "self", file: target }
        : kind === "problem"
          ? { kind: "problem", problem: target, adapter }
          : { kind: "library_checker", problem: target, adapter };
    const existing = payload()?.detail.meta.verify ?? [];
    newVerifyTarget.set("");
    newVerifyAdapter.set("");
    saveSnippetMeta({ verify: [...existing, nextSpec] });
  }

  function removeVerifySpec(idx: number) {
    const existing = payload()?.detail.meta.verify ?? [];
    saveSnippetMeta({ verify: existing.filter((_, i) => i !== idx) });
  }

  function flushSave(targetSlug?: string) {
    clearTimeout(autoSaveTimer);
    autoSaveTimer = undefined;
    const s = targetSlug ?? slug();
    if (!s || info().readOnly) return;
    if (!dirtyCode() && !dirtyNotes()) return;
    let wrote = false;
    if (dirtyCode()) {
      const r = preview.writeSnippetFile(s, "code.hpp", codeText());
      if (r.ok) {
        dirtyCode.set(false);
        wrote = true;
      } else if (r.error) {
        showToast({ tone: "issues", text: r.error });
      }
    }
    if (dirtyNotes()) {
      const r = preview.writeSnippetFile(s, "notes.md", notesText());
      if (r.ok) {
        dirtyNotes.set(false);
        wrote = true;
      } else if (r.error) {
        showToast({ tone: "issues", text: r.error });
      }
    }
    if (wrote) {
      bumpSnippetsRevision();
      loadDetail(s, false);
      canvasCtrl?.compile();
    }
  }
  cleanup(registerQuitFlusher(() => {
    flushSave();
    return true;
  }));

  function scheduleAutoSaveAndCompile() {
    clearTimeout(compileTimer);
    compileTimer = setTimeout(() => {
      canvasCtrl?.compile();
    }, 80);
    clearTimeout(autoSaveTimer);
    autoSaveTimer = setTimeout(() => {
      flushSave();
    }, 450);
  }

  function handleBack() {
    flushSave();
    closeSnippetStudio();
  }

  let lastSlug = "";
  effect(() => {
    const s = slug();
    void revision();
    void snippetsRevision();
    if (!s) return;
    const changed = s !== lastSlug;
    if (changed && lastSlug) {
      flushSave(lastSlug);
    }
    lastSlug = s;
    if (changed) {
      dirtyCode.set(false);
      dirtyNotes.set(false);
    }
    loadDetail(s, changed);
    if (!changed) {
      canvasCtrl?.compile();
    }
  });

  cleanup(() => {
    flushSave();
    clearTimeout(verifySpinTimer);
    clearTimeout(autoSaveTimer);
    clearTimeout(compileTimer);
  });

  function copySingleFile() {
    const s = slug();
    if (!s) return;
    flushSave(s);
    const res = preview.bundleSnippetJson(s, false);
    const parsed = parseJsonPayload<{ code: string }>(res);
    if (parsed?.code) {
      void Clipboard.writeText(parsed.code);
      copiedFlag.trigger();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  }

  function runVerify(mode: "single" | "cascade") {
    const s = slug();
    if (!s) return;
    flushSave(s);
    runSnippetVerifyJob({
      mode,
      slug: s,
      force: true,
      onStart: () => verifying.set(true),
      onFinish: () => {
        verifying.set(false);
        loadDetail(s, false);
      },
    });
  }

  keymap.bind(
    {
      escape: () => props.active && handleBack(),
      "mod+s": () => props.active && flushSave(),
      "mod+i": () => props.active && infoOpen.set(!infoOpen()),
    },
    { when: "always" },
  );

  const hasCode = () => Boolean(payload()?.detail.code);
  const statusInfo = computed(() => {
    const p = payload();
    if (!p) return undefined;
    return snippetStatusStyle(p.status, Boolean(p.detail.code));
  });

  const previewRequest = computed<PreviewRequest>(() => {
    const s = slug();
    if (!s) return { target: "snippet", id: "", book: "", path: "", text: "" };
    const tpl = previewTemplate();
    if (tab() === "code" && dirtyCode()) {
      return { target: "snippet", id: s, book: tpl, path: `snippets/${s}/code.hpp`, text: codeText() };
    }
    if (tab() === "notes" && dirtyNotes()) {
      return { target: "snippet", id: s, book: tpl, path: `snippets/${s}/notes.md`, text: notesText() };
    }
    return { target: "snippet", id: s, book: tpl, path: "", text: "" };
  });

  const tabItems = computed(() => {
    const list: { value: SnippetTab; label: string }[] = [];
    if (hasCode()) list.push({ value: "code", label: t("snippets.studio.tabCode") });
    list.push({ value: "notes", label: t("snippets.studio.tabNotes") });
    if (hasCode()) list.push({ value: "verify", label: t("snippets.studio.tabVerify") });
    return list;
  });

  const unifiedVerifyRows = computed(() => {
    const specs = payload()?.detail.meta.verify ?? [];
    const cases = payload()?.report?.cases ?? [];
    return specs.map((spec) => {
      const target = spec.kind === "self" ? spec.file : spec.problem;
      const adapter = spec.kind === "self" ? undefined : spec.adapter;
      const matched = cases.find((c) => c.kind === spec.kind && c.target === target);
      return { spec, target, adapter, result: matched };
    });
  });

  const verifyKindEntries: MenuEntry[] = [
    { id: "self", label: t("snippets.verifyKind.self") },
    { id: "library_checker", label: t("snippets.verifyKind.library_checker") },
    { id: "problem", label: t("snippets.verifyKind.problem") },
  ];

  const verifyKindLabel = (k: VerifyKindOption) =>
    k === "self"
      ? t("snippets.verifyKind.self")
      : k === "problem"
        ? t("snippets.verifyKind.problem")
        : t("snippets.verifyKind.library_checker");

  return (
    <View style={{ flex: 1, flexDirection: "column" }} class="bg-background">
      <TitleBar
        borderless
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], minWidth: 0 }}>
            <Btn
              testID="snippet-studio-back"
              icon="arrowLeft"
              variant="ghost"
              tooltip={tip(t("common.back"), "escape")}
              onPress={handleBack}
            />
            <Text
              style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1 }}
              class="text-foreground"
            >
              {payload()?.detail.meta.title ?? slug()}
            </Text>
            <If when={!!statusInfo()}>
              <View
                style={{
                  height: 20, // token-exempt: compact status badge height
                  paddingLeft: SPACE[8],
                  paddingRight: SPACE[8],
                  borderRadius: RADIUS.xs,
                  justifyContent: "center",
                  backgroundColor: statusInfo()!.bg,
                }}
              >
                <Text style={{ fontSize: FONT_SIZE.caption, color: statusInfo()!.fg }}>
                  {t(statusInfo()!.labelKey as MessageKey)}
                </Text>
              </View>
            </If>
          </View>
        }
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
            <If when={hasCode()}>
              <Btn
                variant="outline"
                icon={copiedFlag.active() ? "check" : "copy"}
                label={copiedFlag.active() ? t("snippets.action.copied") : t("snippets.action.copySingleFile")}
                onPress={copySingleFile}
                style={{ width: 112 }}
              />
              <Btn
                variant="primary"
                icon={showVerifySpinner() ? undefined : "play"}
                label={verifying() ? t("snippets.action.verifying") : t("snippets.action.verify")}
                disabled={verifying()}
                onPress={() => runVerify("single")}
                style={{ width: 104 }}
              >
                <DelayedSpinner active={verifying()} size={13} />
              </Btn>
            </If>
            <IconBtn
              testID="snippet-info-toggle"
              icon="panelRight"
              tooltip={tip(t("palette.action.properties"), "mod+i")}
              tone={infoOpen() ? "foreground" : "secondary-foreground"}
              onPress={() => infoOpen.set(!infoOpen())}
            />
          </View>
        }
      />

      <InsetPanel direction="row">
        <ResizablePanelGroup style={{ flex: 1, minWidth: 0 }}>
          <ResizablePanel size={500} minSize={300}>
            <View style={{ flex: 1, flexDirection: "column", borderRightWidth: 1 }} class="border-border bg-card">
              <View
                style={{
                  height: ROW.table,
                  paddingLeft: SPACE[12],
                  paddingRight: SPACE[12],
                  flexDirection: "row",
                  alignItems: "center",
                  borderBottomWidth: 1,
                }}
                class="border-border"
              >
                <Segmented items={tabItems()} value={tab()} onChange={(v) => tab.set(v)} itemWidth={68} underline />
              </View>

              <View style={{ flex: 1, position: "relative" }}>
                <If when={hasCode()}>
                  <View
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      right: 0,
                      bottom: 0,
                      visibility: tab() === "code" ? "visible" : "hidden",
                      pointerEvents: tab() === "code" ? "auto" : "none",
                    }}
                  >
                    <Editor
                      style={{ flex: 1 }}
                      value={codeText()}
                      language="cpp"
                      readOnly={info().readOnly || tab() !== "code"}
                      fontFamily={monoFont()}
                      fontSize={editorFontSize()}
                      lineHeight={Math.round(editorFontSize() * editorLineHeight())}
                      scrollBeyondLastLine={80}
                      onChange={(e) => {
                        codeText.set(e.value);
                        dirtyCode.set(true);
                        scheduleAutoSaveAndCompile();
                      }}
                      classes={{ gutter: "pl-2 pr-1", content: "pl-2 pt-2" }}
                    />
                  </View>
                </If>

                <View
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    right: 0,
                    bottom: 0,
                    visibility: tab() === "notes" ? "visible" : "hidden",
                    pointerEvents: tab() === "notes" ? "auto" : "none",
                  }}
                >
                  <Editor
                    style={{ flex: 1 }}
                    value={notesText()}
                    language="markdown"
                    softWrap
                    readOnly={info().readOnly || tab() !== "notes"}
                    fontFamily={monoFont()}
                    fontSize={editorFontSize()}
                    lineHeight={Math.round(editorFontSize() * editorLineHeight())}
                    scrollBeyondLastLine={80}
                    onChange={(e) => {
                      notesText.set(e.value);
                      dirtyNotes.set(true);
                      scheduleAutoSaveAndCompile();
                    }}
                    classes={{ gutter: "pl-2 pr-1", content: "pl-2 pt-2" }}
                  />
                </View>

                <If when={tab() === "verify"}>
                  <Scroll
                    style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0 }}
                    contentStyle={{ padding: SPACE[16], gap: SPACE[12] }}
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                      <Btn
                        variant="outline"
                        icon="shieldCheck"
                        label={t("snippets.action.verifyCascade")}
                        disabled={verifying()}
                        onPress={() => runVerify("cascade")}
                      />
                    </View>

                    <View style={{ gap: SPACE[8] }}>
                      <If
                        when={unifiedVerifyRows().length > 0}
                        else={
                          <SectionEmpty message={t("snippets.studio.noVerifyCases")} />
                        }
                      >
                        <Each of={unifiedVerifyRows()} key={(row, i) => `${i}:${row.spec.kind}:${row.target}`}>
                          {(row) => {
                            const r = () => row().result;
                            const isUnavail = () => r()?.verdict === "UNAVAILABLE";
                            return (
                              <View
                                style={{
                                  padding: SPACE[12],
                                  borderRadius: RADIUS.lg,
                                  borderWidth: 1,
                                  gap: SPACE[8],
                                  boxShadow: cardShadow(),
                                }}
                                class="border-border bg-background"
                              >
                                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                                  <Chip label={verifyKindLabel(row().spec.kind as VerifyKindOption)} />
                                  <Text
                                    style={{ fontSize: FONT_SIZE.body, fontFamily: row().spec.kind === "self" ? MONO : undefined, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                                    class="text-foreground"
                                  >
                                    {row().adapter ? `${row().target} (${row().adapter})` : row().target}
                                  </Text>
                                  <If when={!!r()}>
                                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                                      <Icon
                                        name={r()!.passed ? "check" : "circleAlert"}
                                        size="sm"
                                        tone={r()!.passed ? "chart-2" : isUnavail() ? "chart-4" : "destructive"}
                                      />
                                      <Text
                                        style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                                        class={r()!.passed ? "text-foreground" : isUnavail() ? "text-secondary-foreground" : "text-destructive"}
                                      >
                                        {isUnavail() ? t("snippets.verify.unavailable") : r()!.verdict}
                                      </Text>
                                      <If when={!isUnavail()}>
                                        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                          {`${r()!.passed_cases}/${r()!.total_cases}`}
                                        </Text>
                                      </If>
                                    </View>
                                  </If>
                                </View>

                                <If when={!!r()?.message}>
                                  <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                                    {r()!.message!}
                                  </Text>
                                </If>

                                <If when={!!r()?.compile_stderr}>
                                  <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, fontFamily: MONO }} class="text-destructive">
                                    {r()!.compile_stderr!}
                                  </Text>
                                </If>

                                <If when={isUnavail()}>
                                  <View style={{ flexDirection: "row", justifyContent: "flex-start" }}>
                                    <Btn
                                      variant="outline"
                                      icon="settings"
                                      label={t("snippets.verify.goSettings")}
                                      onPress={() => {
                                        handleBack();
                                        goSettings("toolchain", { expandAdvanced: true });
                                      }}
                                    />
                                  </View>
                                </If>
                              </View>
                            );
                          }}
                        </Each>
                      </If>
                    </View>
                  </Scroll>
                </If>
              </View>
            </View>
          </ResizablePanel>

          <ResizablePanel size={500} minSize={280}>
            <PreviewCanvas
              request={previewRequest}
              controller={(c) => {
                canvasCtrl = c;
              }}
              hud
              active={props.active}
            />
          </ResizablePanel>
        </ResizablePanelGroup>

        {/* Right collapsible 320pt Info & Verify Specs drawer (review A) */}
        <If when={infoOpen()}>
          <View
            testID="snippet-info-drawer"
            style={{ width: 320, borderLeftWidth: 1, flexDirection: "column" }}
            class="border-border bg-card"
          >
            <View
              style={{
                height: ROW.table,
                paddingLeft: SPACE[12],
                paddingRight: SPACE[12],
                justifyContent: "center",
                borderBottomWidth: 1,
              }}
              class="border-border"
            >
              <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("snippets.trd.propInfo")}
              </Text>
            </View>

            <Scroll style={{ flex: 1 }} contentStyle={{ padding: SPACE[12], gap: SPACE[12] }}>
              <View style={{ gap: SPACE[4] }}>
                <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                  {t("snippets.newModal.name")}
                </Text>
                <Input
                  testID="snippet-meta-title"
                  value={metaTitle()}
                  readonly={info().readOnly}
                  onChange={(e) => metaTitle.set(e.value)}
                  onBlur={() => saveSnippetMeta()}
                  onSubmit={() => saveSnippetMeta()}
                  class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                  style={{ height: CONTROL.lg, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.body }}
                />
              </View>

              <View style={{ gap: SPACE[4] }}>
                <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                  {t("snippets.studio.description")}
                </Text>
                <Input
                  value={metaDesc()}
                  readonly={info().readOnly}
                  onChange={(e) => metaDesc.set(e.value)}
                  onBlur={() => saveSnippetMeta()}
                  onSubmit={() => saveSnippetMeta()}
                  class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                  style={{ height: CONTROL.lg, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.body }}
                />
              </View>

              <View style={{ flexDirection: "row", gap: SPACE[8] }}>
                <View style={{ flex: 1, gap: SPACE[4] }}>
                  <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                    {t("snippets.newModal.timeComplexity")}
                  </Text>
                  <Input
                    value={metaTime()}
                    placeholder="O(n log n)"
                    readonly={info().readOnly}
                    onChange={(e) => metaTime.set(e.value)}
                    onBlur={() => saveSnippetMeta()}
                    onSubmit={() => saveSnippetMeta()}
                    class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring placeholder:text-muted-foreground"
                    style={{ height: CONTROL.lg, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.caption }}
                  />
                </View>
                <View style={{ flex: 1, gap: SPACE[4] }}>
                  <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                    {t("snippets.newModal.spaceComplexity")}
                  </Text>
                  <Input
                    value={metaSpace()}
                    placeholder="O(n)"
                    readonly={info().readOnly}
                    onChange={(e) => metaSpace.set(e.value)}
                    onBlur={() => saveSnippetMeta()}
                    onSubmit={() => saveSnippetMeta()}
                    class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring placeholder:text-muted-foreground"
                    style={{ height: CONTROL.lg, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.caption }}
                  />
                </View>
              </View>

              <View style={{ gap: SPACE[4] }}>
                <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                  {t("snippets.newModal.tags")}
                </Text>
                <Input
                  testID="snippet-meta-tags"
                  value={metaTags()}
                  placeholder={t("snippets.newModal.tagsPlaceholder")}
                  readonly={info().readOnly}
                  onChange={(e) => metaTags.set(e.value)}
                  onBlur={() => saveSnippetMeta()}
                  onSubmit={() => saveSnippetMeta()}
                  class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring placeholder:text-muted-foreground"
                  style={{ height: CONTROL.lg, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.body }}
                />
              </View>

              {/* Verify specs management ([[verify]]) */}
              <If when={hasCode()}>
                <View style={{ paddingTop: SPACE[8], borderTopWidth: 1, gap: SPACE[8] }} class="border-border">
                  <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {t("snippets.studio.verifyCases")}
                  </Text>

                  <Each of={payload()?.detail.meta.verify ?? []} key={(v, i) => `${i}:${v.kind}`}>
                    {(spec, idx) => (
                      <View
                        style={{
                          padding: SPACE[8],
                          borderRadius: RADIUS.md,
                          borderWidth: 1,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[6],
                        }}
                        class="border-border bg-card"
                      >
                        <Chip label={verifyKindLabel(spec().kind as VerifyKindOption)} />
                        <Text
                          style={{ flex: 1, fontSize: FONT_SIZE.caption, fontFamily: spec().kind === "self" ? MONO : undefined, numberOfLines: 1, textOverflow: "ellipsis" }}
                          class="text-foreground"
                        >
                          {spec().kind === "self"
                            ? (spec() as { kind: "self"; file: string }).file
                            : (spec() as { problem: string }).problem}
                        </Text>
                        <If when={!info().readOnly}>
                          <IconBtn
                            icon="x"
                            size="sm"
                            tone="muted-foreground"
                            tooltip={tip(t("snippets.studio.verifyRemove"))}
                            onPress={() => removeVerifySpec(idx())}
                          />
                        </If>
                      </View>
                    )}
                  </Each>

                  <If when={!info().readOnly}>
                    <View style={{ gap: SPACE[6], paddingTop: SPACE[2] }}>
                      <AppMenu
                        entries={verifyKindEntries.map((e) => ({
                          ...e,
                          checked: e.id === newVerifyKind(),
                        }))}
                        onSelect={(id) => newVerifyKind.set(id as VerifyKindOption)}
                        trigger={
                          <View
                            class="bg-card border border-border hover:bg-muted"
                            style={{
                              height: CONTROL.md,
                              paddingLeft: SPACE[12],
                              paddingRight: SPACE[8],
                              borderRadius: RADIUS.sm,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                            }}
                          >
                            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
                              {verifyKindLabel(newVerifyKind())}
                            </Text>
                            <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                          </View>
                        }
                      />
                      <Input
                        value={newVerifyTarget()}
                        placeholder={
                          newVerifyKind() === "self"
                            ? "test.cpp"
                            : newVerifyKind() === "library_checker"
                              ? "convolution_mod"
                              : t("snippets.studio.verifyTarget")
                        }
                        onChange={(e) => newVerifyTarget.set(e.value)}
                        onSubmit={addVerifySpec}
                        class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring placeholder:text-muted-foreground"
                        style={{ height: CONTROL.md, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.caption, fontFamily: newVerifyKind() === "self" ? MONO : undefined }}
                      />
                      <If when={newVerifyKind() !== "self"}>
                        <Input
                          value={newVerifyAdapter()}
                          placeholder="verify.cpp"
                          onChange={(e) => newVerifyAdapter.set(e.value)}
                          onSubmit={addVerifySpec}
                          class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring placeholder:text-muted-foreground"
                          style={{ height: CONTROL.md, borderRadius: RADIUS.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], fontSize: FONT_SIZE.caption, fontFamily: MONO }}
                        />
                      </If>
                      <Btn
                        variant="outline"
                        icon="plus"
                        label={t("snippets.studio.addVerify")}
                        disabled={
                          !newVerifyTarget().trim() ||
                          (newVerifyKind() !== "self" && !newVerifyAdapter().trim())
                        }
                        onPress={addVerifySpec}
                      />
                    </View>
                  </If>
                </View>
              </If>
            </Scroll>
          </View>
        </If>
      </InsetPanel>
    </View>
  );
}
