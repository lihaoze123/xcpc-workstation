// Modal for manually recording a training problem (workspace check-in or standalone log).
// Supports free title, pasted OJ URL, or live workspace problem autocomplete, plus optional
// rating, tags, URL, date, and note, with "Save" or "Save & Write Editorial".

import { cleanup, computed, effect, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { ProblemSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, IconBtn, RatingBadge, Segmented } from "../kit";
import { DialogHeader, Modal } from "../Modal";
import { colorTransition } from "../motion";
import { preview, refresh } from "../store";
import { MONO, popoverShadow } from "../theme";
import type { ManualLogStatus } from "./types";

export interface ManualLogModalProps {
  open: boolean;
  onClose: () => void;
  onSaved: (res: { libraryProblemId?: string; createdInLibrary: boolean; openEditorial: boolean }) => void;
  submitRecord?: (reqJson: string) => {
    ok: boolean;
    libraryProblemId?: string | null;
    createdInLibrary: boolean;
    error?: string | null;
  };
}

function todayDateString(): string {
  const now = new Date();
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function looksLikeUrl(s: string): boolean {
  const trimmed = s.trim().toLowerCase();
  return trimmed.startsWith("http://") || trimmed.startsWith("https://");
}

function parseTagsInput(raw: string): string[] {
  return raw
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function formatDateToRfc3339(dateStr: string): string | null {
  const trimmed = dateStr.trim();
  if (!trimmed) return null;
  if (/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) {
    const today = todayDateString();
    if (trimmed === today) {
      return new Date().toISOString();
    }
    return `${trimmed}T12:00:00Z`;
  }
  return trimmed;
}

export function ManualLogModal(props: ManualLogModalProps) {
  const problemInput = signal("");
  const debouncedProblemInput = signal("");
  const linkedProblemId = signal<string | undefined>(undefined);
  const status = signal<ManualLogStatus>("ac");
  const ratingInput = signal("");
  const tagsInput = signal("");
  const urlInput = signal("");
  const dateInput = signal(todayDateString());
  const noteInput = signal("");
  const showSuggestions = signal(false);
  const errorMsg = signal("");
  const saving = signal(false);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  cleanup(() => clearTimeout(searchTimer));

  const setProblemQuery = (val: string, immediate = false) => {
    problemInput.set(val);
    clearTimeout(searchTimer);
    if (immediate || val === "") {
      debouncedProblemInput.set(val);
      return;
    }
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      debouncedProblemInput.set(val);
    }, 120);
  };

  // Reset form each time the modal opens.
  effect(() => {
    if (props.open) {
      clearTimeout(searchTimer);
      problemInput.set("");
      debouncedProblemInput.set("");
      linkedProblemId.set(undefined);
      status.set("ac");
      ratingInput.set("");
      tagsInput.set("");
      urlInput.set("");
      dateInput.set(todayDateString());
      noteInput.set("");
      showSuggestions.set(false);
      errorMsg.set("");
      saving.set(false);
    }
  });

  const suggestions = computed<ProblemSummaryJs[]>(() => {
    if (!props.open || !showSuggestions() || linkedProblemId()) return [];
    const q = debouncedProblemInput().trim();
    if (looksLikeUrl(q)) return [];
    try {
      const list = preview.searchProblems({
        text: q || null,
        tags: [],
        ratingMin: null,
        ratingMax: null,
        platform: null,
        group: null,
        ungroupedOnly: false,
      });
      return list.slice(0, 6);
    } catch {
      return [];
    }
  });

  const pickWorkspaceProblem = (p: ProblemSummaryJs) => {
    linkedProblemId.set(p.id);
    setProblemQuery(p.title || p.id, true);
    if (p.rating != null) ratingInput.set(String(p.rating));
    if (p.tags.length > 0) tagsInput.set(p.tags.join(", "));
    const firstUrl = p.sources.find((s) => s.url)?.url;
    if (firstUrl && !urlInput().trim()) urlInput.set(firstUrl);
    showSuggestions.set(false);
    errorMsg.set("");
  };

  const clearLinkedProblem = () => {
    linkedProblemId.set(undefined);
    showSuggestions.set(true);
  };

  const statusItems = computed(() => [
    { value: "ac" as const, label: t("training.manualLog.statusAc"), testID: "manual-log-status-ac" },
    { value: "upsolved" as const, label: t("training.manualLog.statusUpsolved"), testID: "manual-log-status-upsolved" },
    { value: "attempted" as const, label: t("training.manualLog.statusAttempted"), testID: "manual-log-status-attempted" },
    { value: "todo" as const, label: t("training.manualLog.statusTodo"), testID: "manual-log-status-todo" },
  ]);

  const submit = (createInLibrary: boolean) => {
    if (saving()) return;
    const rawProblem = problemInput().trim();
    const pid = linkedProblemId();
    const rawUrl = urlInput().trim();

    if (!pid && !rawProblem && !rawUrl) {
      errorMsg.set(t("training.manualLog.errorRequired"));
      return;
    }

    let parsedRating: number | null = null;
    const rawRating = ratingInput().trim();
    if (rawRating.length > 0) {
      const n = Number(rawRating);
      if (!Number.isInteger(n) || n < 0 || n > 5000) {
        errorMsg.set(t("training.manualLog.errorRating"));
        return;
      }
      parsedRating = n;
    }

    const isUrlInProblemField = !pid && looksLikeUrl(rawProblem);
    const effectiveUrl = rawUrl || (isUrlInProblemField ? rawProblem : null);
    const effectiveTitle = pid || isUrlInProblemField ? null : rawProblem || null;
    const parsedTags = parseTagsInput(tagsInput());
    const effectiveAt = formatDateToRfc3339(dateInput());
    const effectiveNote = noteInput().trim() || null;

    const payload: Record<string, unknown> = {
      status: status(),
      create_in_library: createInLibrary,
    };
    if (pid) payload.problem_id = pid;
    if (effectiveTitle) payload.title = effectiveTitle;
    if (effectiveUrl) payload.url = effectiveUrl;
    if (parsedRating !== null) payload.rating = parsedRating;
    if (parsedTags.length > 0) payload.tags = parsedTags;
    if (effectiveAt) payload.at = effectiveAt;
    if (effectiveNote) payload.note = effectiveNote;

    saving.set(true);
    errorMsg.set("");
    try {
      const submitFn = props.submitRecord ?? ((json: string) => preview.trainingLogRecord(json));
      const res = submitFn(JSON.stringify(payload));
      if (!res.ok) {
        errorMsg.set(res.error || t("training.manualLog.errorSaveFailed"));
        return;
      }
      refresh();
      props.onClose();
      props.onSaved({
        libraryProblemId: res.libraryProblemId ?? undefined,
        createdInLibrary: res.createdInLibrary,
        openEditorial: createInLibrary,
      });
    } catch (err) {
      errorMsg.set(err instanceof Error ? err.message : t("training.manualLog.errorSaveFailed"));
    } finally {
      saving.set(false);
    }
  };

  return (
    <Modal open={props.open} onClose={props.onClose} width={520} testID="manual-log-modal">
      <View style={{ padding: 20, gap: 16 }}>
        <DialogHeader
          icon="filePenLine"
          title={t("training.manualLog.title")}
          description={t("training.manualLog.desc")}
        />

        {/* Required: Problem title / URL / workspace picker */}
        <View style={{ gap: 6, position: "relative", zIndex: 20 }}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
            <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
              {t("training.manualLog.problemLabel")}
            </Text>
            <If when={linkedProblemId()}>
              {(pid) => (
                <View
                  testID="manual-log-linked-badge"
                  class="bg-muted"
                  style={{
                    height: 20,
                    paddingLeft: 7,
                    paddingRight: 4,
                    borderRadius: 5,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 4,
                  }}
                >
                  <Icon name="link" size={11} tone="muted-foreground" />
                  <Text style={{ fontSize: 11, fontFamily: MONO }} class="text-secondary-foreground">
                    {pid()}
                  </Text>
                  <IconBtn
                    testID="manual-log-unlink-btn"
                    icon="x"
                    size={16}
                    iconSize={10}
                    tone="muted-foreground"
                    onPress={clearLinkedProblem}
                  />
                </View>
              )}
            </If>
          </View>

          <View style={{ position: "relative", zIndex: 30 }}>
            <View
              class="bg-background border border-border focus-within:border-input"
              style={{
                height: 34,
                borderRadius: 7,
                paddingLeft: 10,
                paddingRight: 8,
                flexDirection: "row",
                alignItems: "center",
                gap: 6,
              }}
            >
              <Input
                testID="manual-log-problem-input"
                class="bg-transparent placeholder:text-muted-foreground"
                style={{ flex: 1, height: 28, fontSize: 13 }}
                value={problemInput()}
                placeholder={t("training.manualLog.problemPlaceholder")}
                autoCapitalize="none"
                autoCorrect={false}
                onFocus={() => {
                  if (!linkedProblemId()) showSuggestions.set(true);
                }}
                onChange={(e) => {
                  const val = e.value;
                  setProblemQuery(val);
                  errorMsg.set("");
                  if (linkedProblemId()) {
                    linkedProblemId.set(undefined);
                  }
                  if (looksLikeUrl(val) && !urlInput().trim()) {
                    urlInput.set(val.trim());
                  }
                  showSuggestions.set(true);
                }}
              />
              <If when={problemInput().length > 0}>
                <IconBtn
                  icon="x"
                  size={18}
                  iconSize={11}
                  tone="muted-foreground"
                  onPress={() => {
                    setProblemQuery("", true);
                    linkedProblemId.set(undefined);
                    showSuggestions.set(true);
                  }}
                />
              </If>
            </View>

            {/* Live workspace problem suggestions (floating overlay: never shifts modal height or input) */}
            <If when={suggestions().length > 0}>
              <View
                testID="manual-log-suggestions"
                class="bg-popover border border-border"
                style={{
                  position: "absolute",
                  top: 38,
                  left: 0,
                  right: 0,
                  zIndex: 50,
                  borderRadius: 8,
                  maxHeight: 168,
                  overflow: "hidden",
                  boxShadow: popoverShadow(),
                }}
              >
                <Scroll style={{ maxHeight: 168 }}>
                  <Each of={suggestions()} key={(p) => p.id}>
                    {(prob, idx) => (
                      <View>
                        <If when={idx() > 0}>
                          <View style={{ height: 1, marginLeft: 10, marginRight: 10 }} class="bg-border" />
                        </If>
                        <View
                          testID={`manual-log-suggestion-${prob().id}`}
                          onPress={() => pickWorkspaceProblem(prob())}
                          class="hover:bg-muted active:bg-accent"
                          style={{
                            paddingTop: 7,
                            paddingBottom: 7,
                            paddingLeft: 10,
                            paddingRight: 10,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 8,
                            ...colorTransition(100),
                          }}
                        >
                          <Text
                            style={{ fontSize: 12.5, fontWeight: 500, flex: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                            class="text-foreground"
                          >
                            {prob().title || prob().id}
                          </Text>
                          <Text style={{ fontSize: 11, fontFamily: MONO, flexShrink: 0 }} class="text-muted-foreground">
                            {prob().id}
                          </Text>
                          <If when={prob().rating != null}>
                            <RatingBadge rating={prob().rating} />
                          </If>
                        </View>
                      </View>
                    )}
                  </Each>
                </Scroll>
              </View>
            </If>
          </View>
        </View>

        {/* Required: Status segmented control */}
        <View style={{ gap: 6 }}>
          <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
            {t("training.manualLog.statusLabel")}
          </Text>
          <Segmented
            items={statusItems()}
            value={status()}
            onChange={(next) => status.set(next)}
            itemWidth={108}
          />
        </View>

        {/* Optional compact grid */}
        <View style={{ gap: 10 }}>
          {/* Row 1: Rating + Date */}
          <View style={{ flexDirection: "row", gap: 10 }}>
            <View style={{ flex: 1, gap: 5 }}>
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {t("training.manualLog.ratingLabel")}
              </Text>
              <View
                class="bg-background border border-border focus-within:border-input"
                style={{ height: 32, borderRadius: 7, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }}
              >
                <Input
                  testID="manual-log-rating-input"
                  class="bg-transparent placeholder:text-muted-foreground"
                  style={{ height: 26, fontSize: 12.5, fontVariant: ["tabular-nums"] }}
                  value={ratingInput()}
                  placeholder={t("training.manualLog.ratingPlaceholder")}
                  onChange={(e) => {
                    ratingInput.set(e.value);
                    errorMsg.set("");
                  }}
                />
              </View>
            </View>

            <View style={{ flex: 1, gap: 5 }}>
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {t("training.manualLog.dateLabel")}
              </Text>
              <View
                class="bg-background border border-border focus-within:border-input"
                style={{ height: 32, borderRadius: 7, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }}
              >
                <Input
                  testID="manual-log-date-input"
                  class="bg-transparent placeholder:text-muted-foreground"
                  style={{ height: 26, fontSize: 12.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                  value={dateInput()}
                  placeholder="YYYY-MM-DD"
                  onChange={(e) => dateInput.set(e.value)}
                />
              </View>
            </View>
          </View>

          {/* Row 2: Tags + Source/URL */}
          <View style={{ flexDirection: "row", gap: 10 }}>
            <View style={{ flex: 1, gap: 5 }}>
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {t("training.manualLog.tagsLabel")}
              </Text>
              <View
                class="bg-background border border-border focus-within:border-input"
                style={{ height: 32, borderRadius: 7, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }}
              >
                <Input
                  testID="manual-log-tags-input"
                  class="bg-transparent placeholder:text-muted-foreground"
                  style={{ height: 26, fontSize: 12.5 }}
                  value={tagsInput()}
                  placeholder={t("training.manualLog.tagsPlaceholder")}
                  autoCapitalize="none"
                  autoCorrect={false}
                  onChange={(e) => tagsInput.set(e.value)}
                />
              </View>
            </View>

            <View style={{ flex: 1, gap: 5 }}>
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {t("training.manualLog.urlLabel")}
              </Text>
              <View
                class="bg-background border border-border focus-within:border-input"
                style={{ height: 32, borderRadius: 7, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }}
              >
                <Input
                  testID="manual-log-url-input"
                  class="bg-transparent placeholder:text-muted-foreground"
                  style={{ height: 26, fontSize: 12.5, fontFamily: MONO }}
                  value={urlInput()}
                  placeholder={t("training.manualLog.urlPlaceholder")}
                  autoCapitalize="none"
                  autoCorrect={false}
                  onChange={(e) => urlInput.set(e.value)}
                />
              </View>
            </View>
          </View>

          {/* Row 3: Note */}
          <View style={{ gap: 5 }}>
            <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
              {t("training.manualLog.noteLabel")}
            </Text>
            <View
              class="bg-background border border-border focus-within:border-input"
              style={{ height: 32, borderRadius: 7, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }}
            >
              <Input
                testID="manual-log-note-input"
                class="bg-transparent placeholder:text-muted-foreground"
                style={{ height: 26, fontSize: 12.5 }}
                value={noteInput()}
                placeholder={t("training.manualLog.notePlaceholder")}
                onChange={(e) => noteInput.set(e.value)}
              />
            </View>
          </View>
        </View>

        {/* Fixed error slot */}
        <If when={errorMsg().length > 0}>
          <Text testID="manual-log-error" style={{ fontSize: 12 }} class="text-destructive">
            {errorMsg()}
          </Text>
        </If>

        {/* Footer actions */}
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: 8, paddingTop: 4 }}>
          <Btn
            testID="manual-log-cancel"
            label={t("common.cancel")}
            variant="ghost"
            disabled={saving()}
            onPress={props.onClose}
          />
          <Btn
            testID="manual-log-save"
            label={t("training.manualLog.save")}
            variant="outline"
            disabled={saving()}
            onPress={() => submit(false)}
          />
          <Btn
            testID="manual-log-save-editorial"
            label={t("training.manualLog.saveAndEditorial")}
            icon="filePenLine"
            variant="primary"
            disabled={saving()}
            onPress={() => submit(true)}
          />
        </View>
      </View>
    </Modal>
  );
}
