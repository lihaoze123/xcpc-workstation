// Shared library table cells, tri-state checkbox, and header renderers (§5.2).
// Reused verbatim by both flat <DataTable> and grouped <GroupedProblemList>.

import { Text, View, type JSX } from "nsh";
import type { ProblemSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Chip, RatingBadge, tip } from "../kit";
import type { CheckboxTriState, LibraryColumnKey } from "../logic";
import { colorTransition } from "../motion";
import { MONO } from "../theme";
import { StatusMark } from "../studio/StatusSlot";
import { markForPackageStatus } from "../studio/statusLogic";

import { FONT_SIZE, RADIUS, SPACE } from "../tokens";
export type LibraryRow = ProblemSummaryJs & {
  ratingText: string;
  tagsText: string;
  source: string;
  status: string;
};

export const ROW_LINE = 20;

/** Compact 16x16 tri-state checkbox matching the design system's 4px radius & primary fill. */
export function TriCheckbox(props: {
  state: CheckboxTriState;
  visible?: boolean | (() => boolean);
  onPress?: () => void;
  testID?: string;
}) {
  const checked = () => props.state !== "none";
  const shown = () => (typeof props.visible === "function" ? props.visible() : (props.visible ?? true));
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
      class={
        checked()
          ? "bg-primary border border-primary hover:opacity-90"
          : "bg-card border border-border hover:border-foreground/40"
      }
      style={{
        width: 16,
        height: 16,
        borderRadius: RADIUS.xs,
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        opacity: shown() ? 1 : 0,
        ...colorTransition(120),
      }}
    >
      {props.state === "all" && <Icon name="check" size="sm" tone="primary-foreground" />}
      {props.state === "partial" && <Icon name="minus" size="sm" tone="primary-foreground" />}
    </View>
  );
}

/** Fixed status icon slot (draws nothing when empty, §6.1). */
function StatusSlot(props: { children?: JSX.Element; tooltipText?: string; compact?: boolean }) {
  const size = props.compact ? 16 : 18;
  return (
    <View
      tooltip={props.tooltipText ? tip(props.tooltipText) : undefined}
      style={{
        width: size,
        height: 18, // token-exempt: compact inline status cell
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      {props.children}
    </View>
  );
}

/** Compact training status icon rendered inside the Status column's first slot (aligned across all rows). */
function TrainingStatusSlot(props: { status?: string | null }) {
  const st = props.status ?? "none";
  if (st === "ac") {
    return (
      <StatusSlot compact tooltipText={t("training.status.ac")}>
        <Icon name="check" size="sm" tone="chart-2" />
      </StatusSlot>
    );
  }
  if (st === "upsolved") {
    return (
      <StatusSlot compact tooltipText={t("training.status.upsolved")}>
        <Icon name="shieldCheck" size="sm" tone="primary" />
      </StatusSlot>
    );
  }
  if (st === "attempted") {
    return (
      <StatusSlot compact tooltipText={t("training.status.attempted")}>
        <Icon name="circleAlert" size="sm" tone="chart-4" />
      </StatusSlot>
    );
  }
  if (st === "todo") {
    return (
      <StatusSlot compact tooltipText={t("training.status.todo")}>
        <View style={{ width: 9, height: 9, borderRadius: RADIUS.xs, borderWidth: 1.5 }} class="border-secondary-foreground" />
      </StatusSlot>
    );
  }
  return <StatusSlot compact />;
}

export function renderLibraryCellContent(
  key: LibraryColumnKey | string,
  row: LibraryRow | undefined,
  opts: {
    selected: boolean;
    showCheckbox?: boolean | (() => boolean);
    showInlineId: boolean;
    maxTags: number;
    tagsWidth?: number;
    trainingEnabled?: boolean;
    onToggleSelect?: () => void;
  },
): JSX.Element | null {
  if (!row) return null;

  if (key === "select") {
    return (
      <TriCheckbox
        state={opts.selected ? "all" : "none"}
        visible={opts.showCheckbox}
        onPress={opts.onToggleSelect}
      />
    );
  }

  if (key === "title") {
    return (
      <View style={{ flex: 1, flexDirection: "row", alignItems: "center", gap: SPACE[6], minWidth: 0 }}>
        <Text
          style={{
            numberOfLines: 1,
            textOverflow: "ellipsis",
            fontSize: FONT_SIZE.body,
            lineHeight: ROW_LINE,
            flexShrink: 1,
            minWidth: 0,
          }}
          class="text-foreground"
        >
          {row.title}
        </Text>
        {opts.showInlineId && (
          <Text
            style={{
              numberOfLines: 1,
              textOverflow: "ellipsis",
              fontSize: FONT_SIZE.caption,
              lineHeight: ROW_LINE,
              fontFamily: MONO,
              maxWidth: 112,
              flexShrink: 1,
              minWidth: 0,
            }}
            class="text-muted-foreground"
          >
            {row.id}
          </Text>
        )}
      </View>
    );
  }

  if (key === "ratingText") {
    return row.rating != null ? <RatingBadge rating={row.rating} /> : null;
  }

  if (key === "tagsText") {
    // Budget tags against the column's inner width so `+N` is never clipped off-edge.
    const maxChipW = 78;
    const budget = Math.max(72, (opts.tagsWidth ?? 176) - 20);
    const visible: string[] = [];
    let used = 0;
    for (let i = 0; i < row.tags.length && visible.length < opts.maxTags; i++) {
      const tag = row.tags[i];
      const est = Math.min(maxChipW, Math.ceil(tag.length * 6.8) + 14);
      const gap = visible.length > 0 ? 4 : 0;
      const remainingAfter = row.tags.length - (i + 1);
      const overflowReserve = remainingAfter > 0 ? 30 : 0;
      if (visible.length > 0 && used + gap + est + overflowReserve > budget) break;
      visible.push(tag);
      used += gap + est;
    }
    const overflow = Math.max(0, row.tags.length - visible.length);
    return (
      <View style={{ flex: 1, flexDirection: "row", alignItems: "center", gap: SPACE[4], minWidth: 0, overflow: "hidden" }}>
        <Each of={visible} key={(tag) => tag}>
          {(tag) => <Chip label={tag()} maxWidth={maxChipW} />}
        </Each>
        {overflow > 0 && (
          <View
            style={{
              height: 20, // token-exempt: compact inline chip height
              paddingLeft: SPACE[4],
              paddingRight: SPACE[4],
              borderRadius: RADIUS.xs,
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
            class="bg-muted"
          >
            <Text style={{ fontSize: FONT_SIZE.micro, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {`+${overflow}`}
            </Text>
          </View>
        )}
      </View>
    );
  }

  if (key === "status") {
    const st = row.packageStatus;
    const tr = opts.trainingEnabled ? row.trainingStatus : undefined;
    const summaryParts: string[] = [];
    if (st === "verified_fresh") summaryParts.push(t("library.status.verified"));
    else if (st === "failed") summaryParts.push(t("library.status.failed"));
    else if (st === "stale") summaryParts.push(t("library.status.stale"));
    else if (st === "unverified") summaryParts.push(t("library.status.unverified"));
    if (tr === "ac") summaryParts.push(t("training.status.ac"));
    else if (tr === "upsolved") summaryParts.push(t("training.status.upsolved"));
    else if (tr === "attempted") summaryParts.push(t("training.status.attempted"));
    else if (tr === "todo") summaryParts.push(t("training.status.todo"));
    if (row.hasEditorial) summaryParts.push(t("library.status.hasEditorial"));
    if (row.hasSolutions) summaryParts.push(t("library.status.hasCode"));
    const combinedTip = summaryParts.length > 0 ? summaryParts.join("  ") : undefined;

    // Show the single highest-priority status indicator; secondary details live in the tooltip.
    let primary: JSX.Element | undefined;
    if (st === "failed" || st === "stale") {
      primary = <StatusMark kind={markForPackageStatus(st)} />;
    } else if (tr && tr !== "none") {
      primary = <TrainingStatusSlot status={tr} />;
    } else if (st) {
      primary = <StatusMark kind={markForPackageStatus(st)} />;
    } else if (row.hasEditorial) {
      primary = <Icon name="filePenLine" size="sm" tone="secondary-foreground" />;
    } else if (row.hasSolutions) {
      primary = <Icon name="code" size="sm" tone="secondary-foreground" />;
    }

    return (
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "center" }}>
        <StatusSlot tooltipText={combinedTip}>{primary}</StatusSlot>
      </View>
    );
  }

  if (key === "source") {
    return (
      <Text
        style={{
          numberOfLines: 1,
          textOverflow: "ellipsis",
          fontSize: FONT_SIZE.caption,
          lineHeight: ROW_LINE,
          fontFamily: MONO,
          flexShrink: 1,
          minWidth: 0,
        }}
        class="text-secondary-foreground"
      >
        {row.source}
      </Text>
    );
  }

  return null;
}

export function cellPadding(key: string): { paddingLeft: number; paddingRight: number; justifyContent: "flex-start" | "center" | "flex-end" } {
  if (key === "select") return { paddingLeft: SPACE[0], paddingRight: SPACE[0], justifyContent: "center" };
  if (key === "ratingText") return { paddingLeft: SPACE[8], paddingRight: SPACE[12], justifyContent: "flex-end" };
  if (key === "status") return { paddingLeft: SPACE[4], paddingRight: SPACE[4], justifyContent: "center" };
  if (key === "title") return { paddingLeft: SPACE[8], paddingRight: SPACE[12], justifyContent: "flex-start" };
  return { paddingLeft: SPACE[12], paddingRight: SPACE[12], justifyContent: "flex-start" };
}

export function renderLibraryHeaderContent(props: {
  key: string;
  title: string;
  sortDir?: "ascending" | "descending" | null;
  triState: () => CheckboxTriState;
  showCheckbox?: boolean | (() => boolean);
  onToggleAll: () => void;
  nativeSortIndicator?: boolean;
}) {
  const pad = cellPadding(props.key);
  const sortable = props.key === "title" || props.key === "ratingText";
  const isEnd = props.key === "ratingText";

  if (props.key === "select") {
    return (
      <View
        style={{
          flex: 1,
          minWidth: 0,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <TriCheckbox
          state={props.triState()}
          visible={props.showCheckbox}
          onPress={props.onToggleAll}
          testID="library-select-all"
        />
      </View>
    );
  }

  const chevron =
    !props.nativeSortIndicator && props.sortDir ? (
      <Icon
        name="chevronDown"
        size="sm"
        tone="secondary-foreground"
        style={{ transform: props.sortDir === "ascending" ? "rotate(180deg)" : "rotate(0deg)" }}
      />
    ) : null;

  return (
    <View
      class={sortable ? "hover:bg-muted/60 transition-colors" : undefined}
      style={{
        flex: 1,
        minWidth: 0,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: pad.justifyContent,
        paddingLeft: pad.paddingLeft,
        paddingRight: props.nativeSortIndicator && sortable ? Math.max(pad.paddingRight, 20) : pad.paddingRight,
        gap: SPACE[4],
      }}
    >
      {isEnd && chevron}
      <Text
        style={{ fontSize: FONT_SIZE.caption, lineHeight: ROW_LINE, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0 }}
        class={props.sortDir ? "text-foreground" : "text-muted-foreground"}
      >
        {props.title}
      </Text>
      {!isEnd && chevron}
    </View>
  );
}
