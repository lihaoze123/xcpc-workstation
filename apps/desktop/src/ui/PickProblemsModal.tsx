// Contest problem picker modal (§4): append-only batch selection from the library with
// live search, tag/rating/source filters, cross-filter selection persistence, 26-problem
// capacity enforcement, and keyboard navigation.

import { batch, cleanup, computed, effect, Scroll, signal, Text, untracked, View, type InputRef, type KeyEvent } from "nsh";
import { Input } from "nsh/base";
import { Btn, Chip, Chevron, RatingBadge, tip } from "./kit";
import { ErrorDisplay } from "./ErrorDisplay";
import { Icon } from "./Icon";
import { EmptyState, Spacer } from "./layout";
import { TIER_RANGES, togglePickProblem, type TierName } from "./logic";
import { AppMenu, type MenuEntry } from "./Menu";
import { DialogHeader } from "./Modal";
import { EASE_STANDARD, MONO, ms } from "./theme";
import { dialog, errorOf, info, preview, refresh, revision, type UiError } from "./store";

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
          class={props.active ? "bg-accent border border-border" : "border border-border bg-card hover:bg-muted active:bg-accent"}
          style={{
            height: 28,
            paddingLeft: 9,
            paddingRight: 7,
            borderRadius: 6,
            flexDirection: "row",
            alignItems: "center",
            gap: 5,
            maxWidth: 140,
            minWidth: 0,
            overflow: "hidden",
            transitionProperty: "colors",
            transitionDuration: ms(120),
            transitionTimingFunction: EASE_STANDARD,
          }}
        >
          <Text
            style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0, fontSize: 12.5 }}
            class={props.active ? "text-foreground" : "text-secondary-foreground"}
          >
            {props.label}
          </Text>
          <Chevron size={12} tone="muted-foreground" />
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

  effect(() => {
    const len = rows().length;
    if (len === 0) highlighted.set(0);
    else if (highlighted() >= len) highlighted.set(len - 1);
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
      error.set(errorOf(res, "加入比赛"));
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
      highlighted.set(Math.min(list.length - 1, highlighted() + 1));
    } else if (e.key === "ArrowUp") {
      highlighted.set(Math.max(0, highlighted() - 1));
    } else if (e.key === " " && !searchFocused) {
      const target = list[highlighted()];
      if (target) toggle(target.id);
    }
  };

  const tagItems = computed<MenuEntry[]>(() =>
    facets().tags.map((t) => ({ id: t, label: t, checked: tags().includes(t) })),
  );
  const tierItems = computed<MenuEntry[]>(() => [
    { id: "", label: "全部难度", checked: tier() === undefined },
    { separator: true, id: "sep" },
    ...TIER_RANGES.map((r) => ({ id: r.tier, label: r.label, checked: tier() === r.tier })),
  ]);
  const platformItems = computed<MenuEntry[]>(() => [
    { id: "", label: "全部来源", checked: platform() === undefined },
    ...(facets().platforms.length > 0 ? [{ separator: true as const, id: "sep" }] : []),
    ...facets().platforms.map((p) => ({ id: p, label: p, checked: platform() === p })),
  ]);

  const remainingAfterPick = () => Math.max(0, remaining() - picked().length);

  return (
    <View
      testID="pick-problems-body"
      captureKeys={["up", "down", "escape"]}
      onKeyDown={onKeyDown}
      style={{ maxHeight: 560 }}
    >
      {/* Header */}
      <View style={{ padding: 18, paddingBottom: 12, gap: 12, borderBottomWidth: 1 }} class="border-border">
        <DialogHeader
          icon="listPlus"
          title="从题库挑题"
          description={contest() ? `加入「${contest()!.name}」末尾` : "选择题目加入比赛末尾"}
        />

        {/* Filter bar (hidden when library is completely empty) */}
        <If when={totalProblems() > 0}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <View
              class="border border-border bg-card focus-within:border-primary focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring"
              style={{
                flex: 1,
                minWidth: 140,
                height: 28,
                paddingLeft: 8,
                paddingRight: 6,
                borderRadius: 6,
                flexDirection: "row",
                alignItems: "center",
                gap: 6,
              }}
            >
              <Icon name="search" size={13} tone="muted-foreground" />
              <Input
                ref={(r) => (searchInput = r)}
                style={{ flex: 1, height: 24, fontSize: 12.5 }}
                value={inputText()}
                placeholder="搜索标题、ID、标签或来源"
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
                  tooltip={tip("清除搜索", "escape")}
                  class="hover:bg-muted active:bg-accent"
                  style={{ width: 18, height: 18, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
                >
                  <Icon name="x" size={11} tone="muted-foreground" />
                </View>
              </If>
            </View>
            <PickerFilterMenu
              label={tags().length === 0 ? "标签" : tags().length === 1 ? tags()[0] : `${tags().length} 标签`}
              active={tags().length > 0}
              multi
              entries={tagItems().length > 0 ? tagItems() : [{ id: "__empty__", label: "没有标签", disabled: true }]}
              onSelect={(id) => id && id !== "__empty__" && tags.set(tags().includes(id) ? tags().filter((t) => t !== id) : [...tags(), id])}
            />
            <PickerFilterMenu
              label={TIER_RANGES.find((r) => r.tier === tier())?.label ?? "难度"}
              active={tier() !== undefined}
              entries={tierItems()}
              onSelect={(id) => tier.set(id ? (id as TierName) : undefined)}
            />
            <PickerFilterMenu
              label={platform() ?? "来源"}
              active={platform() !== undefined}
              entries={platformItems()}
              onSelect={(id) => id !== "__empty__" && platform.set(id || undefined)}
            />
            <If when={filtered()}>
              <Btn label="清除" icon="filterX" variant="ghost" onPress={clearFilters} />
            </If>
          </View>
        </If>
      </View>

      {/* Problem list */}
      <If
        when={totalProblems() > 0}
        else={
          <View style={{ height: 240 }}>
            <EmptyState
              icon="libraryBig"
              title="题库还没有题目"
              hint="先新建一道题目，建好后会自动加入这场比赛"
              action={
                <Btn
                  label="新建题目加入"
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
            <View style={{ height: 220 }}>
              <EmptyState
                icon="searchX"
                title="没有符合条件的题目"
                hint="试试更换关键词或清除筛选条件"
                action={<Btn label="清除筛选" icon="filterX" onPress={clearFilters} />}
              />
            </View>
          }
        >
          <Scroll style={{ maxHeight: 340 }}>
            <View style={{ padding: 6, gap: 2 }}>
              <Each of={rows()} key={(p) => p.id}>
                {(p, idx) => {
                  const inContest = () => existingIds().has(p().id);
                  const checked = () => picked().includes(p().id);
                  const capReached = () => !checked() && picked().length >= remaining();
                  const disabled = () => inContest() || capReached();
                  const focused = () => !disabled() && highlighted() === idx();
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
                            ? "bg-accent"
                            : "hover:bg-muted active:bg-accent"
                      }
                      style={{
                        height: 38,
                        paddingLeft: 10,
                        paddingRight: 10,
                        borderRadius: 6,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 10,
                        opacity: disabled() ? 0.48 : 1,
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
                          borderRadius: 4,
                          alignItems: "center",
                          justifyContent: "center",
                          flexShrink: 0,
                        }}
                      >
                        <If when={checked() || inContest()}>
                          <Icon name="check" size={11} tone={checked() ? "primary-foreground" : "muted-foreground"} />
                        </If>
                      </View>
                      {/* Monospace ID */}
                      <Text
                        style={{ width: 108, fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 0 }}
                        class="text-muted-foreground"
                      >
                        {p().id}
                      </Text>
                      {/* Title */}
                      <Text
                        style={{ flex: 1, minWidth: 0, fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class={disabled() ? "text-muted-foreground" : "text-foreground"}
                      >
                        {p().title}
                      </Text>
                      {/* Compact tags (up to 2) */}
                      <If when={p().tags.length > 0}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 4, maxWidth: 132, overflow: "hidden", flexShrink: 0 }}>
                          <Each of={p().tags.slice(0, 2)} key={(t) => t}>
                            {(t) => <Chip label={t()} maxWidth={64} />}
                          </Each>
                        </View>
                      </If>
                      {/* Rating badge */}
                      <RatingBadge rating={p().rating} />
                      {/* Already in contest indicator */}
                      <If when={inContest()}>
                        <Text style={{ fontSize: 11.5, flexShrink: 0 }} class="text-muted-foreground">
                          已加入
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
        <View style={{ paddingLeft: 18, paddingRight: 18, paddingTop: 8 }}>
          <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
        </View>
      </If>

      {/* Footer */}
      <View
        style={{
          paddingLeft: 18,
          paddingRight: 18,
          paddingTop: 12,
          paddingBottom: 14,
          borderTopWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          gap: 10,
        }}
        class="border-border bg-muted/30"
      >
        <If
          when={picked().length > 0}
          else={
            <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {`还可加入 ${remaining()} 题`}
            </Text>
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <Text style={{ fontSize: 12.5, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
              {`已选 ${picked().length} 题`}
            </Text>
            <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {`剩余名额 ${remainingAfterPick()}`}
            </Text>
            <Btn label="清空已选" variant="ghost" onPress={() => picked.set([])} />
          </View>
        </If>
        <Spacer />
        <Btn label="取消" variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn
          testID="pick-problems-submit"
          variant="primary"
          label={picked().length > 0 ? `加入 ${picked().length} 题` : "加入"}
          disabled={picked().length === 0}
          onPress={submit}
          style={{ minWidth: 80 }}
        />
      </View>
    </View>
  );
}
