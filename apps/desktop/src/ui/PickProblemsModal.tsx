// Contest problem picker modal (§4): append-only batch selection from the library with
// live search, tag/rating/source filters, cross-filter selection persistence, 26-problem
// capacity enforcement, and keyboard navigation.

import {
  batch,
  cleanup,
  computed,
  effect,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  type InputRef,
  type KeyEvent,
  type ScrollRef,
} from "nsh";
import { Input } from "nsh/base";
import { Btn, Chip, Chevron, FOCUS_RING, RatingBadge, tip } from "./kit";
import { t } from "./i18n";
import { ErrorDisplay } from "./ErrorDisplay";
import { Icon } from "./Icon";
import { EmptyState, FilterEmpty, Spacer } from "./layout";
import { TIER_RANGES, togglePickProblem, type TierName } from "./logic";
import { AppMenu, type MenuEntry } from "./Menu";
import { DialogHeader } from "./Modal";
import { EASE_STANDARD, ms } from "./theme";
import { dialog, errorOf, info, preview, refresh, revision, type UiError } from "./store";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "./tokens";
const PICK_ROW_HEIGHT = 38;
const PICK_ROW_GAP = 2;
const PICK_ROW_STRIDE = PICK_ROW_HEIGHT + PICK_ROW_GAP;
const PICK_LIST_HEIGHT = 340;

/** Find the next enabled row index moving by `delta` (+1 or -1), skipping disabled rows. */
export function nextEnabledIndex<T>(
  rows: readonly T[],
  current: number,
  delta: 1 | -1,
  isDisabled: (row: T) => boolean,
): number {
  if (rows.length === 0) return -1;
  let i = current + delta;
  while (i >= 0 && i < rows.length) {
    if (!isDisabled(rows[i])) return i;
    i += delta;
  }
  return current >= 0 && current < rows.length && !isDisabled(rows[current]) ? current : -1;
}

function PickerFilterMenu(props: {
  label: string;
  active: boolean;
  entries: MenuEntry[];
  multi?: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <AppMenu
      entries={props.entries}
      onSelect={props.onSelect}
      stayOpen={props.multi}
      minWidth={160}
      trigger={
        <View
          class={
            props.active
              ? `bg-accent border border-border ${FOCUS_RING}`
              : `border border-border bg-card hover:bg-muted active:bg-accent ${FOCUS_RING}`
          }
          style={{
            height: CONTROL.md,
            paddingLeft: SPACE[8],
            paddingRight: SPACE[8],
            borderRadius: RADIUS.sm,
            flexDirection: "row",
            alignItems: "center",
            gap: SPACE[4],
            maxWidth: 112,
            minWidth: 0,
            overflow: "hidden",
            transitionProperty: "colors",
            transitionDuration: ms(120),
            transitionTimingFunction: EASE_STANDARD,
          }}
        >
          <Text
            style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0, fontSize: FONT_SIZE.caption }}
            class={props.active ? "text-foreground" : "text-secondary-foreground"}
          >
            {props.label}
          </Text>
          <Chevron size="sm" tone="muted-foreground" />
        </View>
      }
    />
  );
}

export function PickProblemsBody(props: { contestId: string }) {
  const inputText = signal("");
  const query = signal("");
  const tags = signal<string[]>([]);
  const tier = signal<TierName | undefined>(undefined);
  const platform = signal<string | undefined>(undefined);
  const picked = signal<string[]>([]);
  const highlighted = signal(0);
  const error = signal<UiError | undefined>(undefined);
  let searchInput: InputRef | undefined;
  let listScroll: ScrollRef | undefined;
  let listHeight = PICK_LIST_HEIGHT;
  let listScrollY = 0;
  let searchFocused = false;
  let debounceTimer: number | undefined;

  cleanup(() => clearTimeout(debounceTimer));

  // Reset state whenever opened for a different contest.
  effect(() => {
    void props.contestId;
    untracked(() => {
      batch(() => {
        inputText.set("");
        query.set("");
        tags.set([]);
        tier.set(undefined);
        platform.set(undefined);
        picked.set([]);
        highlighted.set(0);
        error.set(undefined);
      });
    });
  });

  const setSearch = (v: string) => {
    inputText.set(v);
    clearTimeout(debounceTimer);
    if (v === "") {
      query.set("");
      return;
    }
    debounceTimer = setTimeout(() => query.set(v), 120);
  };

  const contest = computed(() => info().contests.find((c) => c.id === props.contestId));
  const existingIds = computed(() => new Set((contest()?.items ?? []).map((i) => i.id)));
  const remaining = computed(() => Math.max(0, 26 - existingIds().size));
  const totalProblems = computed(() => info().problems.length);

  const facets = computed(() => {
    void revision();
    return preview.workspaceFacets();
  });

  const rows = computed(() => {
    void revision();
    const r = TIER_RANGES.find((x) => x.tier === tier());
    return preview.searchProblems({
      text: query().trim() || null,
      tags: tags(),
      ratingMin: r ? r.min : null,
      ratingMax: r && r.max !== null ? r.max - 1 : null,
      platform: platform() ?? null,
    });
  });

  const isRowDisabled = (id: string) => {
    if (existingIds().has(id)) return true;
    const sel = picked();
    return !sel.includes(id) && sel.length >= remaining();
  };

  function ensureVisible(index: number) {
    if (!listScroll || listHeight <= 0 || index < 0) return;
    const rowTop = 6 + index * PICK_ROW_STRIDE;
    const rowBottom = rowTop + PICK_ROW_HEIGHT;
    if (rowTop < listScrollY + 6) {
      listScroll.scrollTo({ y: Math.max(0, rowTop - 6), animated: false });
    } else if (rowBottom > listScrollY + listHeight - 6) {
      listScroll.scrollTo({ y: Math.max(0, rowBottom - listHeight + 6), animated: false });
    }
  }

  effect(() => {
    const list = rows();
    if (list.length === 0) {
      highlighted.set(-1);
      return;
    }
    const cur = untracked(highlighted);
    if (cur < 0 || cur >= list.length || isRowDisabled(list[cur].id)) {
      const firstEnabled = list.findIndex((p) => !isRowDisabled(p.id));
      highlighted.set(firstEnabled);
      if (firstEnabled >= 0) ensureVisible(firstEnabled);
    }
  });

  const filtered = () => inputText().trim() !== "" || tags().length > 0 || tier() !== undefined || platform() !== undefined;
  const clearFilters = () => {
    clearTimeout(debounceTimer);
    batch(() => {
      inputText.set("");
      query.set("");
      tags.set([]);
      tier.set(undefined);
      platform.set(undefined);
    });
  };

  const toggle = (id: string) => {
    error.set(undefined);
    picked.set(togglePickProblem(picked(), id, existingIds(), 26));
  };

  const submit = () => {
    const c = contest();
    const toAdd = picked();
    if (!c || toAdd.length === 0) return;
    const nextIds = [...c.items.map((i) => i.id), ...toAdd];
    const res = preview.composeContest(c.id, nextIds);
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.addToContest")));
      return;
    }
    refresh();
    dialog.set(undefined);
  };

  const onKeyDown = (e: KeyEvent) => {
    if (e.key === "Escape") {
      if (inputText().length > 0) {
        setSearch("");
        searchInput?.focus();
      } else {
        dialog.set(undefined);
      }
      return;
    }
    if ((e.modifiers.platform || e.modifiers.control) && e.key === "Enter") {
      submit();
      return;
    }
    const list = rows();
    if (list.length === 0) return;
    if (e.key === "ArrowDown") {
      const next = nextEnabledIndex(list, highlighted(), 1, (p) => isRowDisabled(p.id));
      if (next >= 0) {
        highlighted.set(next);
        ensureVisible(next);
      }
    } else if (e.key === "ArrowUp") {
      const next = nextEnabledIndex(list, highlighted(), -1, (p) => isRowDisabled(p.id));
      if (next >= 0) {
        highlighted.set(next);
        ensureVisible(next);
      }
    } else if (e.key === "Enter") {
      const target = list[highlighted()];
      if (target && !isRowDisabled(target.id)) toggle(target.id);
    } else if (e.key === " " && (!searchFocused || inputText().length === 0)) {
      const target = list[highlighted()];
      if (target && !isRowDisabled(target.id)) toggle(target.id);
    }
  };

  const tagItems = computed<MenuEntry[]>(() =>
    facets().tags.map((t) => ({ id: t, label: t, checked: tags().includes(t) })),
  );
  const tierItems = computed<MenuEntry[]>(() => [
    { id: "", label: t("library.filter.allRatings"), checked: tier() === undefined },
    { separator: true, id: "sep" },
    ...TIER_RANGES.map((r) => ({ id: r.tier, label: r.label, checked: tier() === r.tier })),
  ]);
  const platformItems = computed<MenuEntry[]>(() => [
    { id: "", label: t("library.filter.allSources"), checked: platform() === undefined },
    ...(facets().platforms.length > 0 ? [{ separator: true as const, id: "sep" }] : []),
    ...facets().platforms.map((p) => ({ id: p, label: p, checked: platform() === p })),
  ]);

  const remainingAfterPick = () => Math.max(0, remaining() - picked().length);

  return (
    <View
      testID="pick-problems-body"
      captureKeys={["up", "down", "escape", "enter"]}
      onKeyDown={onKeyDown}
      style={{ maxHeight: 560 }}
    >
      {/* Header */}
      <View style={{ padding: SPACE[16], paddingBottom: SPACE[12], gap: SPACE[12], borderBottomWidth: 1 }} class="border-border">
        <DialogHeader
          icon="listPlus"
          title={t("pickProblems.title")}
          description={contest() ? t("pickProblems.descContest", { name: contest()!.name }) : t("pickProblems.descGeneric")}
        />

        {/* Filter bar (hidden when library is completely empty) */}
        <If when={totalProblems() > 0}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: SPACE[6] }}>
            <View
              class="border border-border bg-card focus-within:border-primary focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring"
              style={{
                flex: 1,
                minWidth: 140,
                height: CONTROL.md,
                paddingLeft: SPACE[8],
                paddingRight: SPACE[6],
                borderRadius: RADIUS.sm,
                flexDirection: "row",
                alignItems: "center",
                gap: SPACE[6],
              }}
            >
              <Icon name="search" size="sm" tone="muted-foreground" />
              <Input
                ref={(r) => (searchInput = r)}
                style={{ flex: 1, height: CONTROL.sm, fontSize: FONT_SIZE.caption }}
                value={inputText()}
                placeholder={t("pickProblems.searchPlaceholder")}
                autoCapitalize="none"
                autoCorrect={false}
                autoFocus
                onFocus={() => (searchFocused = true)}
                onBlur={() => (searchFocused = false)}
                onChange={(e) => setSearch(e.value)}
              />
              <If when={inputText().length > 0}>
                <View
                  onPress={() => {
                    setSearch("");
                    searchInput?.focus();
                  }}
                  tooltip={tip(t("library.clearSearch"), "escape")}
                  class={`hover:bg-muted active:bg-accent ${FOCUS_RING}`}
                  style={{ width: 18, height: 18, borderRadius: RADIUS.xs, alignItems: "center", justifyContent: "center" }} // token-exempt: compact checkbox square
                >
                  <Icon name="x" size="sm" tone="muted-foreground" />
                </View>
              </If>
            </View>
            <PickerFilterMenu
              label={tags().length === 0 ? t("library.filter.tags") : tags().length === 1 ? tags()[0] : t("library.filter.tagsCount", { count: tags().length })}
              active={tags().length > 0}
              multi
              entries={tagItems().length > 0 ? tagItems() : [{ id: "__empty__", label: t("library.filter.noTags"), disabled: true }]}
              onSelect={(id) => id && id !== "__empty__" && tags.set(tags().includes(id) ? tags().filter((t) => t !== id) : [...tags(), id])}
            />
            <PickerFilterMenu
              label={TIER_RANGES.find((r) => r.tier === tier())?.label ?? t("library.filter.rating")}
              active={tier() !== undefined}
              entries={tierItems()}
              onSelect={(id) => tier.set(id ? (id as TierName) : undefined)}
            />
            <PickerFilterMenu
              label={platform() ?? t("library.filter.source")}
              active={platform() !== undefined}
              entries={platformItems()}
              onSelect={(id) => id !== "__empty__" && platform.set(id || undefined)}
            />
            <If when={filtered()}>
              <Btn label={t("common.clear")} icon="filterX" variant="ghost" onPress={clearFilters} />
            </If>
          </View>
        </If>
      </View>

      {/* Problem list (fixed height when library non-empty to prevent modal vertical jitter) */}
      <If
        when={totalProblems() > 0}
        else={
          <View style={{ height: 240 }}>
            <EmptyState
              icon="libraryBig"
              title={t("pickProblems.emptyLibraryTitle")}
              hint={t("pickProblems.emptyLibraryHint")}
              action={
                <Btn
                  label={t("contests.newProblemAdd")}
                  icon="plus"
                  variant="primary"
                  onPress={() => dialog.set({ kind: "newProblem", addTo: props.contestId })}
                />
              }
            />
          </View>
        }
      >
        <If
          when={rows().length > 0}
          else={
            <View style={{ height: PICK_LIST_HEIGHT }}>
              <FilterEmpty message={t("library.emptyFilteredTitle")} onClear={clearFilters} />
            </View>
          }
        >
          <Scroll
            ref={(r) => (listScroll = r)}
            style={{ height: PICK_LIST_HEIGHT }}
            onLayout={(e) => (listHeight = e.bounds.size.height)}
            onScroll={(e) => (listScrollY = e.offset.y)}
          >
            <View style={{ padding: SPACE[6], gap: PICK_ROW_GAP }}>
              <Each of={rows()} key={(p) => p.id}>
                {(p, idx) => {
                  const inContest = () => existingIds().has(p().id);
                  const checked = () => picked().includes(p().id);
                  const capReached = () => !checked() && picked().length >= remaining();
                  const disabled = () => inContest() || capReached();
                  const focused = () => !disabled() && highlighted() === idx();
                  const visibleTags = () => p().tags.slice(0, 2);
                  return (
                    <View
                      testID={`pick-row-${p().id}`}
                      disabled={disabled()}
                      onPress={() => {
                        if (!disabled()) {
                          highlighted.set(idx());
                          toggle(p().id);
                        }
                      }}
                      onMouseMove={() => {
                        if (!disabled() && highlighted() !== idx()) highlighted.set(idx());
                      }}
                      class={
                        disabled()
                          ? ""
                          : focused()
                            ? `bg-accent ${FOCUS_RING}`
                            : `hover:bg-muted active:bg-accent ${FOCUS_RING}`
                      }
                      style={{
                        height: PICK_ROW_HEIGHT,
                        paddingLeft: SPACE[12],
                        paddingRight: SPACE[12],
                        borderRadius: RADIUS.sm,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[8],
                        opacity: disabled() ? 0.58 : 1,
                        transitionProperty: "colors",
                        transitionDuration: ms(100),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      {/* Checkbox */}
                      <View
                        class={
                          checked()
                            ? "bg-primary border border-primary"
                            : inContest()
                              ? "bg-muted border border-border"
                              : "bg-card border border-border"
                        }
                        style={{
                          width: 16,
                          height: 16,
                          borderRadius: RADIUS.xs,
                          alignItems: "center",
                          justifyContent: "center",
                          flexShrink: 0,
                        }}
                      >
                        <If when={checked() || inContest()}>
                          <Icon name="check" size="sm" tone={checked() ? "primary-foreground" : "muted-foreground"} />
                        </If>
                      </View>
                      {/* Problem ID */}
                      <Text
                        style={{ width: 108, fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 0 }}
                        class="text-muted-foreground"
                      >
                        {p().id}
                      </Text>
                      {/* Title */}
                      <Text
                        style={{ flex: 1, minWidth: 0, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class={disabled() ? "text-muted-foreground" : "text-foreground"}
                      >
                        {p().title}
                      </Text>
                      {/* Compact tags (up to 2, single tag uses full 132pt without artificial 64pt clipping) */}
                      <If when={visibleTags().length > 0}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], maxWidth: 136, overflow: "hidden", flexShrink: 0 }}>
                          <Each of={visibleTags()} key={(t) => t}>
                            {(t) => (
                              <Chip
                                label={t()}
                                maxWidth={visibleTags().length === 1 ? 132 : 96}
                                tooltip={tip(t())}
                              />
                            )}
                          </Each>
                        </View>
                      </If>
                      {/* Rating badge */}
                      <RatingBadge rating={p().rating} />
                      {/* Already in contest indicator */}
                      <If when={inContest()}>
                        <Text style={{ fontSize: FONT_SIZE.caption, flexShrink: 0 }} class="text-muted-foreground">
                          {t("pickProblems.alreadyAdded")}
                        </Text>
                      </If>
                    </View>
                  );
                }}
              </Each>
            </View>
          </Scroll>
        </If>
      </If>

      {/* Error banner */}
      <If when={error()}>
        <View style={{ paddingLeft: SPACE[16], paddingRight: SPACE[16], paddingTop: SPACE[8] }}>
          <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
        </View>
      </If>

      {/* Footer */}
      <View
        style={{
          paddingLeft: SPACE[16],
          paddingRight: SPACE[16],
          paddingTop: SPACE[12],
          paddingBottom: SPACE[12],
          borderTopWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          gap: SPACE[8],
        }}
        class="border-border bg-muted/30"
      >
        <If
          when={picked().length > 0}
          else={
            <If when={remaining() === 0}>
              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {t("pickProblems.capReached")}
              </Text>
            </If>
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
              {t("pickProblems.selectedCount", { count: picked().length })}
            </Text>
            <If when={remainingAfterPick() === 0}>
              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {t("pickProblems.capReached")}
              </Text>
            </If>
            <Btn label={t("pickProblems.clearSelected")} variant="ghost" onPress={() => picked.set([])} />
          </View>
        </If>
        <Spacer />
        <Btn label={t("common.cancel")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn
          testID="pick-problems-submit"
          variant="primary"
          label={picked().length > 0 ? t("pickProblems.submitCount", { count: picked().length }) : t("pickProblems.submit")}
          disabled={picked().length === 0}
          onPress={submit}
          style={{ minWidth: 80 }}
        />
      </View>
    </View>
  );
}
