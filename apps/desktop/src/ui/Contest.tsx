// What the contests space and the studio share about a contest: its composition
// statistics (the 8-tier bar and tag counts), finalizing (定稿), and the confirmation
// before editing a finalized one again, listing what changed since (GUI only, never MCP).

import { cleanup, computed, Scroll, signal, Text, View } from "nsh";
import type { PreviewDiagnostic } from "xcpc-workstation/native";
import { t } from "./i18n";
import { Btn, Chip, tip } from "./kit";
import { ErrorDisplay } from "./ErrorDisplay";
import { DialogHeader, Modal } from "./Modal";
import { diffKind, TIER_RANGES } from "./logic";
import { MONO, tierColor, type Tier } from "./theme";
import { activeRevision, errorOf, info, preview, refresh, unfreezeFor, type UiError } from "./store";

const TIER_RANGE_LABEL: Record<string, string> = Object.fromEntries(
  TIER_RANGES.map((r) => [r.tier, r.label]),
);

function tierLabel(tier: string): string {
  if (tier === "unrated") return t("contests.stats.unrated");
  return TIER_RANGE_LABEL[tier] ?? tier;
}

/** The 8-tier distribution as one bar with its legend, and the tags by count. */
export function ContestStats(props: { id: string; compact?: boolean; active?: boolean }) {
  const rev = activeRevision(() => props.active ?? true);
  const stats = computed(() => {
    void rev();
    return preview.contestStats(props.id);
  });
  const total = () => Math.max(1, stats().problemCount);
  const label = (text: string) => (
    <Text style={{ width: 56, fontSize: 12, lineHeight: 18 }} class="text-muted-foreground">
      {text}
    </Text>
  );
  const bar = (
    <View style={props.compact ? { gap: 8, minWidth: 0 } : { gap: 8, flex: 1, minWidth: 0 }}>
      <View style={{ height: 8, flexDirection: "row", borderRadius: 4, overflow: "hidden", gap: 2 }} class="bg-muted">
        <Each of={stats().ratingDistribution.filter((b) => b.count > 0)} key={(b) => b.tier}>
          {(b) => <View style={{ flexGrow: b().count / total(), flexBasis: 0, backgroundColor: tierColor(b().tier as Tier) }} />}
        </Each>
      </View>
      <View style={{ flexDirection: "row", flexWrap: "wrap", columnGap: 14, rowGap: 4 }}>
        <Each of={stats().ratingDistribution.filter((b) => b.count > 0)} key={(b) => b.tier}>
          {(b) => (
            <View
              tooltip={props.compact ? tip(tierLabel(b().tier)) : undefined}
              style={{ flexDirection: "row", alignItems: "center", gap: 5 }}
            >
              <View style={{ width: 7, height: 7, borderRadius: 2, backgroundColor: tierColor(b().tier as Tier) }} />
              <If when={!props.compact}>
                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {tierLabel(b().tier)}
                </Text>
              </If>
              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"], fontWeight: 500 }}>
                {t("contests.problemCount", { count: b().count })}
              </Text>
            </View>
          )}
        </Each>
      </View>
    </View>
  );
  const tags = (
    <View
      style={
        props.compact
          ? { flexDirection: "row", flexWrap: "wrap", gap: 4, minWidth: 0 }
          : { flex: 1, minWidth: 0, flexDirection: "row", flexWrap: "wrap", gap: 4 }
      }
    >
      <Each of={stats().tagCounts.slice(0, props.compact ? 8 : 24)} key={(t) => t.tag}>
        {(t) => <Chip label={`${t().tag} ${t().count}`} />}
      </Each>
    </View>
  );
  if (props.compact) {
    return (
      <View style={{ gap: 10 }}>
        {bar}
        <If when={stats().tagCounts.length > 0}>{tags}</If>
      </View>
    );
  }
  return (
    <View style={{ gap: 14 }}>
      <View style={{ flexDirection: "row", gap: 12 }}>
        {label(t("contests.stats.difficulty"))}
        {bar}
      </View>
      <If when={stats().tagCounts.length > 0}>
        <View style={{ flexDirection: "row", gap: 12 }}>
          {label(t("contests.stats.tags"))}
          {tags}
        </View>
      </If>
    </View>
  );
}

/** Freezing a contest: in flight, delayed visibility (>=150 ms), and what stopped it (preflight findings, a rule). */
export function createFreeze() {
  const busy = signal(false);
  const busyVisible = signal(false);
  const error = signal<UiError | undefined>(undefined);
  const findings = signal<PreviewDiagnostic[]>([]);
  let timer: number | undefined;
  cleanup(() => clearTimeout(timer));

  function reset() {
    error.set(undefined);
    findings.set([]);
  }

  async function freeze(id: string) {
    if (busy()) return;
    busy.set(true);
    busyVisible.set(false);
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (busy()) busyVisible.set(true);
    }, 150);
    error.set(undefined);
    findings.set([]);
    const res = await preview.freezeContest(id);
    clearTimeout(timer);
    busy.set(false);
    busyVisible.set(false);
    if (!res.ok || !res.frozen) {
      const blocking = res.diagnostics.filter((d) => d.severity === "error");
      findings.set(blocking.length > 0 ? blocking : res.diagnostics);
      error.set(
        res.code
          ? errorOf({ ok: false, code: res.code, error: res.error }, t("ctx.freeze"))
          : { context: t("ctx.freeze"), error: res.error, code: blocking.length > 0 ? "preflight" : undefined },
      );
    }
    refresh();
  }
  return { busy, busyVisible, error, findings, freeze, reset };
}

export type Freeze = ReturnType<typeof createFreeze>;

/** The message for a freeze stopped by preflight findings. */
export function freezeBlockedCopy(n: number): string {
  return n === 1 ? t("contests.freezeBlockedOne") : t("contests.freezeBlockedMany", { count: n });
}

/** The confirmation before unfreezing: what differs between the snapshot and the library. */
export function UnfreezeDialog() {
  const error = signal<UiError | undefined>(undefined);
  const diff = computed(() => {
    const id = unfreezeFor();
    return id ? preview.unfreezeDiff(id) : undefined;
  });
  const diffFailed = () => diff() !== undefined && !diff()!.ok;
  const name = () => info().contests.find((c) => c.id === unfreezeFor())?.name ?? unfreezeFor() ?? "";
  const close = () => {
    error.set(undefined);
    unfreezeFor.set(undefined);
  };
  const confirm = () => {
    if (diffFailed() || info().readOnly) return;
    const id = unfreezeFor();
    if (!id) return;
    const res = preview.unfreezeContest(id);
    if (!res.ok) {
      error.set(errorOf(res, t("ctx.unfreeze")));
      return;
    }
    refresh();
    close();
  };
  const displayedError = () => error() ?? (diffFailed() ? { code: diff()!.code, error: diff()!.error } : undefined);
  return (
    <Modal open={unfreezeFor() !== undefined} onClose={close} width={460} testID="dialog-unfreeze">
      <View style={{ padding: 20, gap: 16 }}>
        <DialogHeader
          icon="lockOpen"
          title={t("contests.unfreeze.title", { name: name() })}
          description={
            diffFailed()
              ? t("contests.unfreeze.diffFailed")
              : (diff()?.diffs.length ?? 0) > 0
                ? t("contests.unfreeze.hasDiff")
                : t("contests.unfreeze.noDiff")
          }
        />
        <If when={!diffFailed() && (diff()?.diffs.length ?? 0) > 0}>
          <View style={{ marginLeft: 44, borderRadius: 8, overflow: "hidden" }} class="bg-muted">
            <Scroll style={{ maxHeight: 220 }}>
              <View style={{ gap: 8, padding: 10 }}>
                <Each of={diff()?.diffs ?? []} key={(d) => d.label}>
                  {(d) => (
                    <View style={{ gap: 2 }}>
                      <View style={{ flexDirection: "row", gap: 8 }}>
                        <Text style={{ fontSize: 13, fontVariant: ["tabular-nums"], width: 16, fontWeight: 600 }}>{d().label}</Text>
                        <Text style={{ fontSize: 13, flex: 1 }}>{d().kinds.map(diffKind).join(t("diff.listSeparator"))}</Text>
                      </View>
                      <Each of={d().changedFiles.slice(0, 6)} key={(f) => f}>
                        {(f) => (
                          <Text
                            style={{ fontSize: 12, fontFamily: MONO, marginLeft: 24, numberOfLines: 1, textOverflow: "ellipsis" }}
                            class="text-muted-foreground"
                          >
                            {f()}
                          </Text>
                        )}
                      </Each>
                      <If when={d().changedFiles.length > 6}>
                        <Text style={{ fontSize: 11.5, marginLeft: 24, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {t("contests.unfreeze.moreFiles", { count: d().changedFiles.length })}
                        </Text>
                      </If>
                    </View>
                  )}
                </Each>
              </View>
            </Scroll>
          </View>
        </If>
        <If when={displayedError()}>
          <ErrorDisplay error={displayedError()} />
        </If>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Btn label={t("common.cancel")} variant="ghost" onPress={close} />
          <Btn label={t("contests.reopen")} variant="primary" disabled={diffFailed() || info().readOnly} onPress={confirm} testID="confirm-unfreeze" />
        </View>
      </View>
    </Modal>
  );
}
