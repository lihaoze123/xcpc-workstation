// Lightweight single-file quick-editor modal for package source files (`.cpp`, `problem.toml`,
// self-test `.in` files). Reuses NSH's plain monospace `<Editor>` with dirty dot, `Mod+S` /
// Save button, and an "Open External" escape hatch — zero LSP/IDE bloat.

import { effect, keymap, signal, Text, untracked, View } from "nsh";
import { Editor } from "nsh/base";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, IconBtn, tip } from "../kit";
import { Modal } from "../Modal";
import { errorOf, preview, type UiError } from "../store";
import { editorFontSize, editorLineHeight, editorMono, MONO } from "../theme";
import { formatBytesCompact } from "./buildLogic";

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
  const initialText = signal("");
  const sizeBytes = signal(0);
  const tooLarge = signal(false);
  const dirty = signal(false);
  const err = signal<UiError | undefined>(undefined);
  let draftText = "";

  effect(() => {
    const open = props.open;
    const rel = props.relPath;
    const slug = props.problemId;
    untracked(() => {
      if (!open || !rel) {
        loadedPath.set(undefined);
        dirty.set(false);
        err.set(undefined);
        return;
      }
      const res = preview.readPackageFile(slug, rel);
      if (!res.ok) {
        loadedPath.set(rel);
        tooLarge.set(false);
        initialText.set("");
        draftText = "";
        dirty.set(false);
        err.set({ code: res.code, error: res.error, context: t("ctx.openPackageFile") });
        return;
      }
      loadedPath.set(rel);
      sizeBytes.set(res.sizeBytes);
      tooLarge.set(res.tooLarge);
      initialText.set(res.content);
      draftText = res.content;
      dirty.set(false);
      err.set(undefined);
    });
  });

  function saveFile() {
    const rel = untracked(() => props.relPath);
    if (!props.open || !rel || props.readOnly || untracked(tooLarge)) return;
    const res = preview.savePackageFile(props.problemId, rel, draftText);
    if (!res.ok) {
      err.set(errorOf(res, t("ctx.savePackageFile")));
      return;
    }
    err.set(undefined);
    dirty.set(false);
    initialText.set(draftText);
    props.onSaved();
  }

  keymap.bind(
    "mod+s",
    () => {
      if (props.open && !props.readOnly) saveFile();
    },
    { when: "always" },
  );

  return (
    <Modal open={props.open} onClose={props.onClose} width={760} testID="package-quick-edit-modal">
      <View style={{ height: 540 }}>
        {/* Header */}
        <View
          style={{
            height: 44,
            paddingLeft: 14,
            paddingRight: 10,
            borderBottomWidth: 1,
            flexDirection: "row",
            alignItems: "center",
            gap: 8,
          }}
          class="border-border bg-background/50"
        >
          <Icon name="code" size={15} tone="secondary-foreground" />
          <Text
            style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 13, fontWeight: 600, fontFamily: MONO }}
            class="text-foreground"
          >
            {props.relPath ?? ""}
          </Text>
          <If when={dirty()}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
              <View style={{ width: 6, height: 6, borderRadius: 3 }} class="bg-chart-4" />
              <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                {t("build.quickEdit.unsaved")}
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
              onPress={saveFile}
            />
          </If>
          <IconBtn icon="x" size={26} iconSize={14} onPress={props.onClose} />
        </View>

        <If when={err()}>
          <View style={{ padding: 12, borderBottomWidth: 1 }} class="border-border bg-muted/30">
            <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
          </View>
        </If>

        {/* Body */}
        <If
          when={!tooLarge()}
          else={
            <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 28, gap: 12 }}>
              <Icon name="fileText" size={24} tone="muted-foreground" />
              <Text style={{ fontSize: 13, textAlign: "center" }} class="text-secondary-foreground">
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
            <Each of={loadedPath() ? [loadedPath()!] : []} key={(k) => `${props.problemId}:${k}`}>
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
    </Modal>
  );
}
