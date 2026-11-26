// Lightweight single-file quick-editor modal for package source files (`.cpp`, `problem.toml`,
// self-test `.in` files). Reuses NSH's plain monospace `<Editor>` with dirty dot, `Mod+S` /
// Save button, SHA-256 external-edit conflict detection (Fix A-3), and an "Open External" escape hatch.

import { effect, keymap, signal, Text, untracked, View } from "nsh";
import { Editor } from "nsh/base";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, createTransientFlag, IconBtn, tip } from "../kit";
import { Modal } from "../Modal";
import { errorOf, preview, type UiError } from "../store";
import { editorFontSize, editorLineHeight, editorMono, MONO } from "../theme";
import { formatBytesCompact } from "./buildLogic";

import { FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
export function PackageQuickEditModal(props: {
  open: boolean;
  problemId: string;
  relPath: string | undefined;
  readOnly: boolean;
  onClose: () => void;
  onSaved: () => void;
  onOpenExternal: (relPath: string) => void;
}) {
  const loadedPath = signal<string | undefined>(undefined);
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

  function loadFile(slug: string, rel: string) {
    const res = preview.readPackageFile(slug, rel);
    if (!res.ok) {
      loadedPath.set(rel);
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
    loadedPath.set(rel);
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
    const open = props.open;
    const rel = props.relPath;
    const slug = props.problemId;
    untracked(() => {
      if (!open || !rel) {
        loadedPath.set(undefined);
        dirty.set(false);
        conflict.set(false);
        err.set(undefined);
        return;
      }
      loadFile(slug, rel);
    });
  });

  function saveFile(forceOverwrite = false) {
    const rel = untracked(() => props.relPath);
    if (!props.open || !rel || props.readOnly || untracked(tooLarge)) return;
    const expected = forceOverwrite ? null : (untracked(baseHash) ?? null);
    const res = preview.savePackageFile(props.problemId, rel, draftText, expected);
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
      if (props.open && !props.readOnly) saveFile(false);
    },
    { when: "always" },
  );

  return (
    <Modal open={props.open} onClose={props.onClose} width={760} testID="package-quick-edit-modal">
      <View style={{ height: 540 }}>
        {/* Header — always displays target relative path */}
        <View
          style={{
            height: 44,
            paddingLeft: SPACE[12],
            paddingRight: SPACE[12],
            borderBottomWidth: 1,
            flexDirection: "row",
            alignItems: "center",
            gap: SPACE[8],
          }}
          class="border-border bg-background/50"
        >
          <Icon name="code" size="md" tone="secondary-foreground" />
          <Text
            style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontFamily: MONO }}
            class="text-foreground"
          >
            {props.relPath ?? ""}
          </Text>
          <If when={dirty()}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
              <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs }} class="bg-chart-4" />
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                {t("build.quickEdit.unsaved")}
              </Text>
            </View>
          </If>
          <If when={savedFeedback.active() && !dirty()}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
              <Icon name="check" size="sm" tone="secondary-foreground" />
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                {t("build.quickEdit.saved")}
              </Text>
            </View>
          </If>
          <If when={props.relPath}>
            {(rel) => (
              <Btn
                testID="quick-edit-external-btn"
                label={t("build.quickEdit.openExternal")}
                icon="folderOpen"
                variant="outline"
                onPress={() => props.onOpenExternal(rel())}
              />
            )}
          </If>
          <If when={!props.readOnly && !tooLarge()}>
            <Btn
              testID="quick-edit-save-btn"
              label={t("build.quickEdit.save")}
              icon="check"
              variant="primary"
              disabled={!dirty()}
              tooltip={tip(t("build.quickEdit.save"), "mod+s")}
              onPress={() => saveFile(false)}
            />
          </If>
          <IconBtn icon="x" size="sm" onPress={props.onClose} />
        </View>

        {/* Fix A-3: Inline external-edit conflict bar */}
        <If when={conflict()}>
          <View
            testID="quick-edit-conflict-bar"
            style={{
              paddingLeft: SPACE[12],
              paddingRight: SPACE[12],
              paddingTop: SPACE[8],
              paddingBottom: SPACE[8],
              borderBottomWidth: 1,
              borderLeftWidth: 3,
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "space-between",
              gap: SPACE[8],
            }}
            class="border-b-border border-l-chart-4 bg-muted/40"
          >
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flex: 1, minWidth: 0 }}>
              <Icon name="triangleAlert" size="sm" tone="chart-4" />
              <Text style={{ fontSize: FONT_SIZE.caption, flex: 1 }} class="text-foreground">
                {t("build.quickEdit.conflictNotice")}
              </Text>
            </View>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
              <Btn
                testID="quick-edit-reload-btn"
                label={t("build.quickEdit.loadExternal")}
                variant="outline"
                onPress={() => {
                  const rel = props.relPath;
                  if (rel) loadFile(props.problemId, rel);
                }}
              />
              <Btn
                testID="quick-edit-overwrite-btn"
                label={t("build.quickEdit.overwriteSave")}
                variant="danger"
                onPress={() => saveFile(true)}
              />
            </View>
          </View>
        </If>

        <If when={err()}>
          <View style={{ padding: SPACE[12], borderBottomWidth: 1 }} class="border-border bg-muted/30">
            <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
          </View>
        </If>

        {/* Body */}
        <If
          when={!tooLarge()}
          else={
            <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: SPACE[32], gap: SPACE[12] }}>
              <Icon name="fileText" size="lg" tone="muted-foreground" />
              <Text style={{ fontSize: FONT_SIZE.body, textAlign: "center" }} class="text-secondary-foreground">
                {`${t("build.quickEdit.tooLarge")} (${formatBytesCompact(sizeBytes())})`}
              </Text>
              <If when={props.relPath}>
                {(rel) => (
                  <Btn
                    label={t("build.quickEdit.openExternal")}
                    icon="folderOpen"
                    variant="primary"
                    onPress={() => props.onOpenExternal(rel())}
                  />
                )}
              </If>
            </View>
          }
        >
          <View style={{ flex: 1, minHeight: 0 }}>
            <Each of={loadedPath() ? [`${props.problemId}:${loadedPath()!}:${loadSeq()}`] : []} key={(k) => k}>
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
                  fontFamily={editorMono() === "monospace" ? editorMono() : `'JetBrains Mono NL', ${editorMono()}`}
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
    </Modal>
  );
}
