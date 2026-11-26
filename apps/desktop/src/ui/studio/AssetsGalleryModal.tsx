// Centered lightweight assets gallery (§4.0): two states — grid ⇄ enlarged lightbox —
// reusing `Modal`'s centered warm-physical spring reveal. Lists problem `assets/`
// asynchronously via Core `listProblemAssets`, with middle-ellipsized filenames,
// hover-only floating actions (zero reflow), exact Markdown reference jump, Core-backed
// width presets, batch unused cleanup, and full keyboard navigation.

import {
  cleanup,
  colorScheme,
  computed,
  effect,
  Image,
  keymap,
  pixelRatio,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  windowSize,
  type InputRef,
} from "nsh";
import { Input } from "nsh/base";
import type { ActionResult, CleanUnusedAssetsResultJs, ProblemAssetJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, createTransientValue, FOCUS_RING, IconBtn, tip } from "../kit";
import { EmptyState, PageLoading } from "../layout";
import { Modal } from "../Modal";
import { colorTransition, isReducedMotion, overlayCloseMs, overlayTransform, overlayTransition, panelTransition } from "../tokens";
import { errorOf, preview, revision, type UiError } from "../store";
import { MONO, token } from "../theme";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE, TOOLBAR } from "../tokens";
type DeleteConfirm = { kind: "single"; asset: ProblemAssetJs } | { kind: "unused" };

const WIDTH_PRESETS = [50, 75, 100] as const;
const TILE_WIDTH = 168;
const TILE_GAP = 12;

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function middleEllipsis(name: string, maxChars = 16): string {
  if (name.length <= maxChars) return name;
  const dot = name.lastIndexOf(".");
  if (dot > 0 && name.length - dot <= 6) {
    const ext = name.slice(dot);
    const keep = Math.max(4, maxChars - ext.length - 1);
    const head = Math.ceil(keep * 0.65);
    const tail = keep - head;
    return `${name.slice(0, head)}…${tail > 0 ? name.slice(dot - tail, dot) : ""}${ext}`;
  }
  const head = Math.ceil((maxChars - 1) * 0.6);
  const tail = maxChars - 1 - head;
  return `${name.slice(0, head)}…${name.slice(name.length - tail)}`;
}

function isPdfAsset(name: string): boolean {
  return name.toLowerCase().endsWith(".pdf");
}

function referenceLabel(file: string, line: number, total: number): string {
  const doc = file.endsWith("editorial.md") ? t("studio.tab.editorial") : t("studio.tab.statement");
  return total > 1
    ? t("assets.refMultiple", { doc, line, total })
    : t("assets.refSingle", { doc, line });
}

export function AssetsGalleryModal(props: {
  open: boolean;
  target: "problem" | "contest";
  id: string;
  label?: string;
  readOnly: boolean;
  hasTypstBlock?: boolean;
  refreshSeq?: number;
  onClose: () => void;
  onImportImage: () => Promise<string | null>;
  onInsertAsset: (relPath: string) => void;
  onSetAssetWidth: (assetName: string, widthPct: number, docPath?: string | null) => ActionResult;
  onRenameAsset: (oldName: string, newName: string) => ActionResult;
  onDeleteAsset: (name: string) => ActionResult;
  onCleanUnused: () => CleanUnusedAssetsResultJs;
  onLocateReference: (file: string, line: number) => void;
}) {
  const assets = signal<ProblemAssetJs[]>([]);
  const loaded = signal(false);
  const localRev = signal(0);
  const mode = signal<"grid" | "lightbox">("grid");
  const lightboxMounted = signal(false);
  const lightboxShown = signal(false);
  const lightboxClosing = signal(false);
  const lightboxAsset = signal<ProblemAssetJs | undefined>(undefined);
  const selectedIndex = signal(0);
  const gridCols = signal(4);
  const renaming = signal<string | undefined>(undefined);
  const renameDraft = signal("");
  const confirmingDelete = signal<DeleteConfirm | undefined>(undefined);
  const cleanedFeedback = createTransientValue<number>(1800);
  const error = signal<UiError | undefined>(undefined);
  const lightboxFailed = signal(false);

  let listSeq = 0;
  let renameInput: InputRef | undefined;
  let lightboxRaf: number | undefined;
  let lightboxCloseTimer: number | undefined;

  // Reset transient state whenever the modal closes or target switches.
  effect(() => {
    const isOpen = props.open;
    void props.target;
    void props.id;
    void props.label;
    untracked(() => {
      if (!isOpen) {
        if (lightboxRaf !== undefined) {
          cancelAnimationFrame(lightboxRaf);
          lightboxRaf = undefined;
        }
        clearTimeout(lightboxCloseTimer);
        mode.set("grid");
        lightboxMounted.set(false);
        lightboxShown.set(false);
        lightboxAsset.set(undefined);
        renaming.set(undefined);
        confirmingDelete.set(undefined);
        error.set(undefined);
        loaded.set(false);
      } else {
        error.set(undefined);
        confirmingDelete.set(undefined);
        renaming.set(undefined);
        loaded.set(false);
      }
    });
  });

  cleanup(() => {
    if (lightboxRaf !== undefined) cancelAnimationFrame(lightboxRaf);
    clearTimeout(lightboxCloseTimer);
  });

  // Load problem assets asynchronously off the UI thread.
  effect(() => {
    if (!props.open) return;
    void revision();
    void localRev();
    void props.refreshSeq;
    const target = props.target;
    const id = props.id;
    const label = target === "contest" ? (props.label ?? null) : null;
    const seq = ++listSeq;
    void preview
      .listProblemAssets(target, id, label)
      .then((list) => {
        if (seq !== listSeq) return;
        assets.set(list);
        loaded.set(true);
        const cur = untracked(selectedIndex);
        if (list.length === 0) {
          selectedIndex.set(0);
          mode.set("grid");
        } else if (cur >= list.length) {
          selectedIndex.set(list.length - 1);
        }
      })
      .catch(() => {
        if (seq === listSeq) {
          assets.set([]);
          loaded.set(true);
        }
      });
  });

  const currentAsset = computed<ProblemAssetJs | undefined>(() => assets()[selectedIndex()]);
  const unusedAssets = computed(() => assets().filter((a) => !a.referenced));
  const unusedCount = computed(() => unusedAssets().length);
  const unusedBytes = computed(() => unusedAssets().reduce((sum, a) => sum + a.bytes, 0));

  // Warm spring reveal/hide for lightbox over the grid (`opacity + scale(0.97→1)`).
  effect(() => {
    const cur = currentAsset();
    const openLightbox = props.open && mode() === "lightbox" && !!cur;
    if (lightboxRaf !== undefined) {
      cancelAnimationFrame(lightboxRaf);
      lightboxRaf = undefined;
    }
    clearTimeout(lightboxCloseTimer);
    if (openLightbox) {
      untracked(() => {
        lightboxClosing.set(false);
        lightboxAsset.set(cur);
        if (!lightboxMounted()) {
          lightboxMounted.set(true);
          if (isReducedMotion()) {
            lightboxShown.set(true);
          } else {
            lightboxShown.set(false);
            lightboxRaf = requestAnimationFrame(() => {
              lightboxRaf = undefined;
              lightboxShown.set(true);
            });
          }
        } else {
          lightboxShown.set(true);
        }
      });
    } else if (untracked(lightboxMounted)) {
      untracked(() => {
        lightboxClosing.set(true);
        lightboxShown.set(false);
        const dur = overlayCloseMs();
        if (dur === 0) {
          lightboxMounted.set(false);
          lightboxClosing.set(false);
          lightboxAsset.set(undefined);
        } else {
          lightboxCloseTimer = setTimeout(() => {
            lightboxMounted.set(false);
            lightboxClosing.set(false);
            lightboxAsset.set(undefined);
          }, dur);
        }
      });
    }
  });

  effect(() => {
    void lightboxAsset()?.absPath;
    untracked(() => lightboxFailed.set(false));
  });

  effect(() => {
    if (renaming()) {
      queueMicrotask(() => renameInput?.focus());
    }
  });

  const modalHeight = computed(() => {
    const h = windowSize().height;
    if (h <= 0) return 560;
    return Math.max(400, Math.min(720, Math.floor(h * 0.72)));
  });

  function startRename(asset: ProblemAssetJs) {
    if (props.readOnly) return;
    confirmingDelete.set(undefined);
    error.set(undefined);
    renameDraft.set(asset.name);
    renaming.set(asset.name);
  }

  function cancelRename() {
    renaming.set(undefined);
  }

  function commitRename(oldName: string) {
    if (props.readOnly) return;
    const next = untracked(renameDraft).trim();
    if (!next || next === oldName) {
      renaming.set(undefined);
      return;
    }
    const res = props.onRenameAsset(oldName, next);
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.renameAsset")));
      return;
    }
    error.set(undefined);
    renaming.set(undefined);
    localRev.set(localRev() + 1);
  }

  function askDelete(asset: ProblemAssetJs) {
    if (props.readOnly) return;
    renaming.set(undefined);
    error.set(undefined);
    confirmingDelete.set({ kind: "single", asset });
  }

  function commitDelete(asset: ProblemAssetJs) {
    if (props.readOnly) return;
    const res = props.onDeleteAsset(asset.name);
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.deleteAsset")));
      confirmingDelete.set(undefined);
      return;
    }
    error.set(undefined);
    confirmingDelete.set(undefined);
    localRev.set(localRev() + 1);
  }

  function askCleanUnused() {
    if (props.readOnly || unusedCount() === 0) return;
    renaming.set(undefined);
    error.set(undefined);
    confirmingDelete.set({ kind: "unused" });
  }

  function commitCleanUnused() {
    if (props.readOnly || unusedCount() === 0) {
      confirmingDelete.set(undefined);
      return;
    }
    confirmingDelete.set(undefined);
    renaming.set(undefined);
    const res = props.onCleanUnused();
    if (res.ok || res.removed.length > 0) {
      localRev.set(localRev() + 1);
    }
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.cleanUnusedAssets")));
      return;
    }
    error.set(undefined);
    if (res.removed.length > 0) {
      cleanedFeedback.trigger(res.removed.length);
    }
  }

  async function handleImport() {
    if (props.readOnly) return;
    error.set(undefined);
    const rel = await props.onImportImage();
    if (!rel) return;
    localRev.set(localRev() + 1);
  }

  function handleSetWidth(asset: ProblemAssetJs, widthPct: number) {
    if (props.readOnly) return;
    const docPath = asset.references[0]?.file ?? null;
    const res = props.onSetAssetWidth(asset.name, widthPct, docPath);
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.setAssetWidth")));
      return;
    }
    error.set(undefined);
    localRev.set(localRev() + 1);
  }

  function insertAndClose(asset: ProblemAssetJs) {
    if (props.readOnly) return;
    props.onInsertAsset(asset.relPath);
    props.onClose();
  }

  function stepSelection(delta: number) {
    const list = untracked(assets);
    if (list.length === 0) return;
    const cur = untracked(selectedIndex);
    const next =
      untracked(mode) === "lightbox"
        ? (cur + delta + list.length) % list.length
        : Math.max(0, Math.min(list.length - 1, cur + delta));
    selectedIndex.set(next);
  }

  function handleCancel() {
    if (untracked(renaming)) {
      renaming.set(undefined);
      return;
    }
    if (untracked(confirmingDelete)) {
      confirmingDelete.set(undefined);
      return;
    }
    if (untracked(mode) === "lightbox") {
      mode.set("grid");
      return;
    }
    props.onClose();
  }

  // Keyboard navigation while gallery modal is open (disabled during inline rename input).
  effect(() => {
    if (!props.open || renaming()) return;
    const unbind = keymap.bind(
      {
        left: () => stepSelection(-1),
        right: () => stepSelection(1),
        up: () => {
          if (untracked(mode) === "grid") stepSelection(-untracked(gridCols));
        },
        down: () => {
          if (untracked(mode) === "grid") stepSelection(untracked(gridCols));
        },
        space: () => {
          if (untracked(confirmingDelete)) return;
          const list = untracked(assets);
          if (list.length === 0) return;
          mode.set(untracked(mode) === "grid" ? "lightbox" : "grid");
        },
        enter: () => {
          // Disable Enter confirmation during delete confirmation to prevent accidental Delete+Enter chains.
          if (untracked(confirmingDelete)) return;
          const asset = untracked(currentAsset);
          if (asset && !props.readOnly) insertAndClose(asset);
        },
        f2: () => {
          if (untracked(confirmingDelete)) return;
          const asset = untracked(currentAsset);
          if (asset && !props.readOnly) startRename(asset);
        },
        delete: () => {
          const del = untracked(confirmingDelete);
          if (del) {
            if (del.kind === "single") commitDelete(del.asset);
            else commitCleanUnused();
            return;
          }
          const asset = untracked(currentAsset);
          if (asset && !props.readOnly) askDelete(asset);
        },
        escape: handleCancel,
      },
      { when: "always" },
    );
    return unbind;
  });

  const checkerboardBg = (): { backgroundColor: string; backgroundImage: string } => {
    const dark = colorScheme() === "dark";
    const base = dark ? token("sidebar") : token("muted");
    const stripe = dark ? "rgba(255,255,255,0.045)" : "rgba(0,0,0,0.04)";
    return {
      backgroundColor: base,
      backgroundImage: `repeating-linear-gradient(90deg, transparent 0px 8px, ${stripe} 8px 16px), repeating-linear-gradient(180deg, transparent 0px 8px, ${stripe} 8px 16px)`,
    };
  };

  return (
    <Modal open={props.open} onClose={handleCancel} width={880} testID="assets-gallery-modal">
      <View style={{ height: modalHeight(), flexDirection: "column" }} class="bg-card">
        {/* Header bar */}
        <View
          style={{
            height: 48,
            paddingLeft: SPACE[16],
            paddingRight: SPACE[12],
            borderBottomWidth: 1,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: SPACE[8],
            flexShrink: 0,
          }}
          class="border-border"
        >
          <If
            when={mode() === "lightbox" && !!currentAsset()}
            else={
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                  {t("assets.title")}
                </Text>
                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {String(assets().length)}
                </Text>
              </View>
            }
          >
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flex: 1, minWidth: 0 }}>
              <Btn
                label={t("common.back")}
                icon="arrowLeft"
                variant="ghost"
                size="sm"
                tooltip={tip(t("assets.backToGrid"), "escape")}
                onPress={() => mode.set("grid")}
              />
              <If
                when={renaming() === currentAsset()?.name}
                else={
                  <Text
                    style={{
                      flex: 1,
                      fontSize: FONT_SIZE.body,
                      fontWeight: FONT_WEIGHT.semibold,
                      numberOfLines: 1,
                      textOverflow: "ellipsis",
                    }}
                    class="text-foreground"
                  >
                    {currentAsset()?.name ?? ""}
                  </Text>
                }
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], flex: 1, maxWidth: 320 }}>
                  <Input
                    ref={(r) => (renameInput = r)}
                    value={renameDraft()}
                    autoCapitalize="none"
                    autoCorrect={false}
                    submitBehavior="submit"
                    onChange={(e) => renameDraft.set(e.value)}
                    onSubmit={() => currentAsset() && commitRename(currentAsset()!.name)}
                    onKeyPress={(e) => {
                      if (e.key === "Escape") cancelRename();
                    }}
                    class="flex-1 h-7 px-2 rounded border border-primary bg-background text-[12.5px] text-foreground"
                    style={{ fontFamily: MONO }}
                  />
                  <IconBtn
                    icon="check"
                    size="sm"
                    tooltip={tip(t("assets.confirmRename"))}
                    onPress={() => currentAsset() && commitRename(currentAsset()!.name)}
                  />
                  <IconBtn icon="x" size="sm" tooltip={tip(t("common.cancel"))} onPress={cancelRename} />
                </View>
              </If>
            </View>
          </If>

          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
            <If when={mode() === "grid"}>
              <If when={cleanedFeedback.value() != null}>
                <View testID="assets-cleaned-feedback" style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], paddingRight: SPACE[4] }}>
                  <Icon name="check" size="sm" tone="secondary-foreground" />
                  <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                    {t("assets.cleanedUnused", { count: cleanedFeedback.value()! })}
                  </Text>
                </View>
              </If>
              <If when={unusedCount() > 0}>
                <Btn
                  testID="assets-clean-unused-btn"
                  label={t("assets.cleanUnused", { count: unusedCount() })}
                  variant="ghost"
                  size="sm"
                  disabled={props.readOnly}
                  tooltip={props.readOnly ? tip(t("assets.readOnlyCannotClean")) : tip(t("assets.cleanUnusedTooltip"))}
                  onPress={askCleanUnused}
                />
              </If>
              <Btn
                testID="assets-import-btn"
                label={t("assets.import")}
                icon="plus"
                variant="outline"
                size="sm"
                disabled={props.readOnly}
                tooltip={props.readOnly ? tip(t("assets.readOnlyCannotImport")) : tip(t("assets.importTooltip"))}
                onPress={() => void handleImport()}
              />
            </If>
            <If when={mode() === "lightbox" && !!currentAsset() && renaming() !== currentAsset()?.name}>
              <IconBtn
                icon="pencil"
                size="sm"
                disabled={props.readOnly}
                tooltip={tip(t("assets.rename"), "f2")}
                onPress={() => currentAsset() && startRename(currentAsset()!)}
              />
              <IconBtn
                icon="trash"
                size="sm"
                disabled={props.readOnly}
                tooltip={tip(t("assets.deleteAsset"), "delete")}
                onPress={() => currentAsset() && askDelete(currentAsset()!)}
              />
            </If>
            <IconBtn icon="x" size="sm" tooltip={tip(t("common.close"), "escape")} onPress={props.onClose} />
          </View>
        </View>

        {/* Error banner */}
        <If when={error()}>
          <View style={{ paddingLeft: SPACE[16], paddingRight: SPACE[16], paddingTop: SPACE[12], flexShrink: 0 }}>
            <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
          </View>
        </If>

        {/* Delete confirmation bar (single asset or batch unused cleanup) */}
        <If when={confirmingDelete()}>
          {(del) => (
            <View
              testID="assets-delete-confirm"
              class="bg-muted/70 border-b border-border"
              style={{
                paddingLeft: SPACE[16],
                paddingRight: SPACE[12],
                paddingTop: SPACE[8],
                paddingBottom: SPACE[8],
                flexDirection: "row",
                alignItems: "center",
                justifyContent: "space-between",
                gap: SPACE[12],
                flexShrink: 0,
              }}
            >
              <Text style={{ flex: 1, fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-foreground">
                {(() => {
                  const cur = del();
                  if (cur.kind === "single") {
                    return cur.asset.referenced
                      ? t("assets.confirmDeleteReferenced", { name: cur.asset.name })
                      : t("assets.confirmDeleteSingle", { name: cur.asset.name });
                  }
                  return props.hasTypstBlock
                    ? t("assets.confirmDeleteUnusedTypst", { count: unusedCount(), size: formatBytes(unusedBytes()) })
                    : t("assets.confirmDeleteUnused", { count: unusedCount(), size: formatBytes(unusedBytes()) });
                })()}
              </Text>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
                <Btn
                  testID="assets-delete-cancel-btn"
                  label={t("common.cancel")}
                  variant="ghost"
                  size="sm"
                  onPress={() => confirmingDelete.set(undefined)}
                />
                <Btn
                  testID="assets-delete-confirm-btn"
                  label={t("common.delete")}
                  variant="danger"
                  size="sm"
                  onPress={() => {
                    const cur = del();
                    if (cur.kind === "single") commitDelete(cur.asset);
                    else commitCleanUnused();
                  }}
                />
              </View>
            </View>
          )}
        </If>

        {/* Body: Grid underneath, Lightbox spring overlay on top */}
        <View style={{ flex: 1, position: "relative", minHeight: 0 }}>
          <Scroll
            style={{ flex: 1, pointerEvents: mode() === "lightbox" ? "none" : "auto" }}
            onLayout={(e) => {
              const w = e.bounds.size.width;
              if (w > 0) {
                gridCols.set(Math.max(1, Math.floor((w - 32 + TILE_GAP) / (TILE_WIDTH + TILE_GAP))));
              }
            }}
          >
            <If
              when={assets().length > 0}
              else={
                <If
                  when={loaded()}
                  else={<PageLoading active={true} style={{ minHeight: 320 }} />}
                >
                  <EmptyState
                    icon="image"
                    title={t("assets.empty")}
                    action={
                      !props.readOnly ? (
                        <Btn label={t("assets.importImage")} icon="plus" variant="primary" size="sm" onPress={() => void handleImport()} />
                      ) : undefined
                    }
                    style={{ minHeight: 320 }}
                  />
                </If>
              }
            >
              <View
                testID="assets-grid"
                style={{
                  padding: SPACE[16],
                  flexDirection: "row",
                  flexWrap: "wrap",
                  gap: TILE_GAP,
                }}
              >
                <Each of={assets()} key={(a) => a.name}>
                  {(asset, index) => (
                    <AssetTile
                      asset={asset()}
                      selected={selectedIndex() === index()}
                      readOnly={props.readOnly}
                      checkerboard={checkerboardBg()}
                      isRenaming={renaming() === asset().name}
                      renameDraft={renameDraft()}
                      onDraftChange={(v) => renameDraft.set(v)}
                      onRenameInputRef={(r) => (renameInput = r)}
                      onSelect={() => selectedIndex.set(index())}
                      onOpenLightbox={() => {
                        selectedIndex.set(index());
                        mode.set("lightbox");
                      }}
                      onInsert={() => insertAndClose(asset())}
                      onStartRename={() => {
                        selectedIndex.set(index());
                        startRename(asset());
                      }}
                      onCancelRename={cancelRename}
                      onCommitRename={() => commitRename(asset().name)}
                      onAskDelete={() => {
                        selectedIndex.set(index());
                        askDelete(asset());
                      }}
                    />
                  )}
                </Each>
              </View>
            </If>
          </Scroll>

          <If when={lightboxMounted() && !!lightboxAsset()}>
            {() => {
              const asset = lightboxAsset()!;
              const ref0 = asset.references[0];
              const showFallback = isPdfAsset(asset.name) || lightboxFailed();
              const dpr = Math.max(1, pixelRatio());
              const maxWidth = asset.width != null && asset.width > 0 ? asset.width / dpr : undefined;
              const maxHeight = asset.height != null && asset.height > 0 ? asset.height / dpr : undefined;
              return (
                <View
                  testID="assets-lightbox"
                  class="bg-card"
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    right: 0,
                    bottom: 0,
                    flexDirection: "column",
                    transformOrigin: "center",
                    opacity: lightboxShown() ? 1 : 0,
                    transform: lightboxClosing() ? overlayTransform(true) : overlayTransform(lightboxShown()),
                    pointerEvents: mode() === "lightbox" ? "auto" : "none",
                    ...overlayTransition(!lightboxClosing()),
                  }}
                >
                  {/* Fit image stage (clamped to natural logical pixel size so small images stay crisp) */}
                  <View
                    style={{
                      flex: 1,
                      padding: SPACE[20],
                      alignItems: "center",
                      justifyContent: "center",
                      position: "relative",
                      minHeight: 0,
                    }}
                  >
                    <View
                      class="border border-border"
                      style={{
                        width: "100%",
                        height: "100%",
                        borderRadius: RADIUS.lg,
                        overflow: "hidden",
                        ...checkerboardBg(),
                        alignItems: "center",
                        justifyContent: "center",
                        padding: SPACE[12],
                      }}
                    >
                      <If
                        when={!showFallback}
                        else={
                          <View style={{ alignItems: "center", gap: SPACE[8] }}>
                            <Icon name="fileText" size="lg" tone="muted-foreground" />
                            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                              {asset.name}
                            </Text>
                          </View>
                        }
                      >
                        <Image
                          src={asset.absPath}
                          onError={() => lightboxFailed.set(true)}
                          style={{
                            width: "100%",
                            height: "100%",
                            maxWidth,
                            maxHeight,
                            objectFit: "contain",
                          }}
                        />
                      </If>
                    </View>

                    {/* Prev / Next arrows */}
                    <If when={assets().length > 1}>
                      <View style={{ position: "absolute", left: 28, top: 0, bottom: 0, justifyContent: "center", pointerEvents: "box-none" }}>
                        <IconBtn
                          icon="chevronLeft"
                          size="lg"
                          tooltip={tip(t("assets.prevImage"), "left")}
                          onPress={() => stepSelection(-1)}
                          style={{ borderRadius: RADIUS.full }}
                        />
                      </View>
                      <View style={{ position: "absolute", right: 28, top: 0, bottom: 0, justifyContent: "center", pointerEvents: "box-none" }}>
                        <IconBtn
                          icon="chevronRight"
                          size="lg"
                          tooltip={tip(t("assets.nextImage"), "right")}
                          onPress={() => stepSelection(1)}
                          style={{ borderRadius: RADIUS.full }}
                        />
                      </View>
                    </If>
                  </View>

                  {/* Bottom info & actions bar (spacing-separated, no middle-dot separators) */}
                  <View
                    style={{
                      height: TOOLBAR,
                      paddingLeft: SPACE[16],
                      paddingRight: SPACE[16],
                      borderTopWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: SPACE[12],
                      flexShrink: 0,
                    }}
                    class="border-border"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12], flex: 1, minWidth: 0 }}>
                      <If when={asset.width != null && asset.height != null}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {`${asset.width}×${asset.height}`}
                        </Text>
                      </If>
                      <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                        {formatBytes(asset.bytes)}
                      </Text>
                      <If
                        when={!!ref0}
                        else={
                          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                            {t("assets.unreferenced")}
                          </Text>
                        }
                      >
                        <View
                          onPress={() => {
                            if (!ref0) return;
                            props.onClose();
                            props.onLocateReference(ref0.file, ref0.line);
                          }}
                          tooltip={tip(t("assets.jumpToReference"))}
                          class={`hover:bg-muted active:bg-accent ${FOCUS_RING}`}
                          style={{
                            paddingLeft: SPACE[6],
                            paddingRight: SPACE[6],
                            height: CONTROL.sm,
                            borderRadius: RADIUS.xs,
                            justifyContent: "center",
                            ...colorTransition(),
                          }}
                        >
                          <Text
                            style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }}
                            class="text-primary"
                          >
                            {ref0 ? referenceLabel(ref0.file, ref0.line, asset.references.length) : ""}
                          </Text>
                        </View>
                      </If>
                    </View>

                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flexShrink: 0 }}>
                      <If when={asset.referenced && !props.readOnly}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
                          <Each of={WIDTH_PRESETS} key={(w) => String(w)}>
                            {(w) => {
                              const active = () => ref0?.widthPct != null && Math.round(ref0.widthPct) === w();
                              return (
                                <View
                                  onPress={() => handleSetWidth(asset, w())}
                                  tooltip={tip(t("assets.setWidth", { pct: w() }))}
                                  class={
                                    active()
                                      ? `bg-accent border border-border ${FOCUS_RING}`
                                      : `bg-muted hover:bg-accent active:bg-accent ${FOCUS_RING}`
                                  }
                                  style={{
                                    height: CONTROL.sm,
                                    paddingLeft: SPACE[8],
                                    paddingRight: SPACE[8],
                                    borderRadius: RADIUS.sm,
                                    alignItems: "center",
                                    justifyContent: "center",
                                    ...colorTransition(),
                                  }}
                                >
                                  <Text
                                    style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], fontWeight: active() ? FONT_WEIGHT.semibold : FONT_WEIGHT.regular }}
                                    class={active() ? "text-foreground" : "text-secondary-foreground"}
                                  >
                                    {`${w()}%`}
                                  </Text>
                                </View>
                              );
                            }}
                          </Each>
                        </View>
                      </If>
                      <Btn
                        testID="assets-lightbox-insert"
                        label={t("assets.insertAtCaret")}
                        variant="primary"
                        size="sm"
                        disabled={props.readOnly}
                        onPress={() => insertAndClose(asset)}
                      />
                    </View>
                  </View>
                </View>
              );
            }}
          </If>
        </View>
      </View>
    </Modal>
  );
}

function AssetTile(props: {
  asset: ProblemAssetJs;
  selected: boolean;
  readOnly: boolean;
  checkerboard: { backgroundColor: string; backgroundImage: string };
  isRenaming: boolean;
  renameDraft: string;
  onDraftChange: (v: string) => void;
  onRenameInputRef: (r: InputRef) => void;
  onSelect: () => void;
  onOpenLightbox: () => void;
  onInsert: () => void;
  onStartRename: () => void;
  onCancelRename: () => void;
  onCommitRename: () => void;
  onAskDelete: () => void;
}) {
  const hovered = signal(false);
  const thumbFallbackToOrig = signal(false);
  const thumbFailed = signal(false);

  const thumbPath = () => props.asset.thumbPath || undefined;
  const imgSrc = () => (!thumbFallbackToOrig() && thumbPath() ? thumbPath()! : props.asset.absPath);
  const showFallback = () => isPdfAsset(props.asset.name) || thumbFailed();

  return (
    <View
      testID={`asset-tile-${props.asset.name}`}
      onMouseEnter={() => hovered.set(true)}
      onMouseLeave={() => hovered.set(false)}
      style={{ width: TILE_WIDTH, gap: SPACE[6] }}
    >
      {/* Thumbnail box with hover-only floating actions (absolute, zero reflow) */}
      <View
        class={props.selected ? "border-foreground/25 ring-1 ring-inset ring-foreground/15" : "border-border"}
        style={{
          width: TILE_WIDTH,
          height: 126,
          borderRadius: RADIUS.md,
          borderWidth: 1,
          overflow: "hidden",
          position: "relative",
          ...props.checkerboard,
          ...colorTransition(),
        }}
      >
        <View
          onPress={props.onOpenLightbox}
          class={FOCUS_RING}
          style={{
            width: "100%",
            height: "100%",
            alignItems: "center",
            justifyContent: "center",
            padding: SPACE[8],
          }}
        >
          <If
            when={!showFallback()}
            else={<Icon name="fileText" size="lg" tone="muted-foreground" />}
          >
            <Image
              src={imgSrc()}
              onError={() => {
                if (!thumbFallbackToOrig() && thumbPath()) {
                  thumbFallbackToOrig.set(true);
                } else {
                  thumbFailed.set(true);
                }
              }}
              style={{ width: "100%", height: "100%", objectFit: "contain" }}
            />
          </If>
        </View>

        {/* Hover floating actions: insert, rename, delete */}
        <If when={!props.readOnly}>
          <View
            class="bg-popover/95 border border-border"
            style={{
              position: "absolute",
              top: 6,
              right: 6,
              flexDirection: "row",
              alignItems: "center",
              gap: SPACE[2],
              padding: SPACE[2],
              borderRadius: RADIUS.sm,
              opacity: hovered() ? 1 : 0,
              pointerEvents: hovered() ? "auto" : "none",
              ...panelTransition(),
            }}
          >
            <IconBtn icon="plus" size="sm" tooltip={tip(t("assets.insertAtCaret"))} onPress={props.onInsert} />
            <IconBtn icon="pencil" size="sm" tooltip={tip(t("assets.rename"), "f2")} onPress={props.onStartRename} />
            <IconBtn icon="trash" size="sm" tooltip={tip(t("assets.deleteAsset"), "delete")} onPress={props.onAskDelete} />
          </View>
        </If>
      </View>

      {/* Single-line filename (middle-ellipsized) + tabular-nums size + quiet unreferenced tag */}
      <If
        when={props.isRenaming}
        else={
          <View onPress={props.onSelect} class={FOCUS_RING} style={{ gap: SPACE[2], paddingLeft: SPACE[2], paddingRight: SPACE[2] }}>
            <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[6] }}>
              <Text
                style={{
                  flex: 1,
                  fontSize: FONT_SIZE.caption,
                  fontWeight: FONT_WEIGHT.semibold,
                  lineHeight: LINE_HEIGHT.caption,
                  numberOfLines: 1,
                  textOverflow: "ellipsis",
                }}
                class="text-foreground"
              >
                {middleEllipsis(props.asset.name, 16)}
              </Text>
              <Text
                style={{ fontSize: FONT_SIZE.micro, lineHeight: LINE_HEIGHT.caption, fontVariant: ["tabular-nums"], flexShrink: 0 }}
                class="text-muted-foreground"
              >
                {formatBytes(props.asset.bytes)}
              </Text>
            </View>
            <If when={!props.asset.referenced}>
              <Text style={{ fontSize: FONT_SIZE.micro, lineHeight: LINE_HEIGHT.micro }} class="text-muted-foreground">
                {t("assets.unreferenced")}
              </Text>
            </If>
          </View>
        }
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[2] }}>
          <Input
            ref={props.onRenameInputRef}
            value={props.renameDraft}
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="submit"
            onChange={(e) => props.onDraftChange(e.value)}
            onSubmit={props.onCommitRename}
            onKeyPress={(e) => {
              if (e.key === "Escape") props.onCancelRename();
            }}
            class="flex-1 h-6 px-1.5 rounded border border-primary bg-background text-[11.5px] text-foreground"
            style={{ fontFamily: MONO }}
          />
          <IconBtn icon="check" size="sm" tooltip={tip(t("common.confirm"))} onPress={props.onCommitRename} />
          <IconBtn icon="x" size="sm" tooltip={tip(t("common.cancel"))} onPress={props.onCancelRename} />
        </View>
      </If>
    </View>
  );
}
