// Shared library list row, tri-state checkbox, and pure list helpers (Phase 6D §6, §8.1).
// Reused verbatim by both flat list and grouped <GroupedProblemList>.

import { signal, Text, View, type JSX } from "nsh";
import type { ProblemSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { FOCUS_RING, RatingBadge, tip } from "../kit";
import type { CheckboxTriState } from "../logic";
import { colorTransition } from "../motion";
import { StatusMark } from "../studio/StatusSlot";
import { markForPackageStatus } from "../studio/statusLogic";
import { RADIUS, ROW, SPACE, TYPE } from "../tokens";

export type LibraryRow = ProblemSummaryJs & {
  ratingText: string;
  tagsText: string;
  source: string;
  status: string;
  updatedAt?: string | number | null;
};

export type LibrarySortKey = "title" | "rating" | "recent";
export const DEFAULT_LIBRARY_SORT: LibrarySortKey = "title";

export const ROW_LINE = 20;

/** Formats up to `maxVisible` tags as compact secondary text with `+N` overflow (§8.1). */
export function formatRowTags(tags: readonly string[], maxVisible = 2): string {
  if (tags.length === 0) return "";
  const shown = tags.slice(0, maxVisible);
  const overflow = tags.length - shown.length;
  return overflow > 0 ? `${shown.join(" · ")} +${overflow}` : shown.join(" · ");
}

/** Sorts library rows by title (default), rating (unrated last), or recent modification/visit. */
export function sortLibraryRows<
  T extends Pick<ProblemSummaryJs, "id" | "title" | "rating"> & { updatedAt?: string | number | null },
>(
  rows: readonly T[],
  sortKey: LibrarySortKey,
  loc: string,
  recentRank?: ReadonlyMap<string, number>,
): T[] {
  const list = [...rows];
  if (sortKey === "rating") {
    list.sort((a, b) => {
      if (a.rating == null && b.rating == null) return a.title.localeCompare(b.title, loc) || a.id.localeCompare(b.id);
      if (a.rating == null) return 1;
      if (b.rating == null) return -1;
      return a.rating - b.rating || a.title.localeCompare(b.title, loc) || a.id.localeCompare(b.id);
    });
    return list;
  }
  if (sortKey === "recent") {
    list.sort((a, b) => {
      const aTime = a.updatedAt != null ? (typeof a.updatedAt === "number" ? a.updatedAt : Date.parse(a.updatedAt)) : NaN;
      const bTime = b.updatedAt != null ? (typeof b.updatedAt === "number" ? b.updatedAt : Date.parse(b.updatedAt)) : NaN;
      if (!Number.isNaN(aTime) || !Number.isNaN(bTime)) {
        if (Number.isNaN(aTime)) return 1;
        if (Number.isNaN(bTime)) return -1;
        if (bTime !== aTime) return bTime - aTime;
      }
      if (recentRank && recentRank.size > 0) {
        const ra = recentRank.get(a.id);
        const rb = recentRank.get(b.id);
        if (ra !== undefined || rb !== undefined) {
          if (ra === undefined) return 1;
          if (rb === undefined) return -1;
          if (ra !== rb) return ra - rb;
        }
      }
      return b.id.localeCompare(a.id);
    });
    return list;
  }
  list.sort((a, b) => a.title.localeCompare(b.title, loc) || a.id.localeCompare(b.id));
  return list;
}

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
          ? `bg-primary border border-primary hover:opacity-90${props.onPress ? ` ${FOCUS_RING}` : ""}`
          : `bg-card border border-border hover:border-foreground/40${props.onPress ? ` ${FOCUS_RING}` : ""}`
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

/** Compact training status icon rendered inside the 16pt Status slot. */
function TrainingStatusIcon(props: { status?: string | null }) {
  const st = props.status ?? "none";
  if (st === "ac") return <Icon name="check" size="sm" tone="chart-2" />;
  if (st === "upsolved") return <Icon name="shieldCheck" size="sm" tone="primary" />;
  if (st === "attempted") return <Icon name="circleAlert" size="sm" tone="chart-4" />;
  if (st === "todo") {
    return (
      <View
        style={{ width: 9, height: 9, borderRadius: RADIUS.xs, borderWidth: 1.5 }}
        class="border-secondary-foreground"
      />
    );
  }
  return null;
}

/** Fixed 16pt status icon slot (draws nothing when empty, §6.1). */
function LibraryStatusIcon(props: { row: LibraryRow; trainingEnabled?: boolean }) {
  const st = props.row.packageStatus;
  const tr = props.trainingEnabled ? props.row.trainingStatus : undefined;
  const summaryParts: string[] = [];
  if (st === "verified_fresh") summaryParts.push(t("library.status.verified"));
  else if (st === "failed") summaryParts.push(t("library.status.failed"));
  else if (st === "stale") summaryParts.push(t("library.status.stale"));
  else if (st === "unverified") summaryParts.push(t("library.status.unverified"));
  if (tr === "ac") summaryParts.push(t("training.status.ac"));
  else if (tr === "upsolved") summaryParts.push(t("training.status.upsolved"));
  else if (tr === "attempted") summaryParts.push(t("training.status.attempted"));
  else if (tr === "todo") summaryParts.push(t("training.status.todo"));
  if (props.row.hasEditorial) summaryParts.push(t("library.status.hasEditorial"));
  if (props.row.hasSolutions) summaryParts.push(t("library.status.hasCode"));
  const combinedTip = summaryParts.length > 0 ? summaryParts.join("  ") : undefined;

  let primary: JSX.Element | null = null;
  if (st === "failed" || st === "stale") {
    primary = <StatusMark kind={markForPackageStatus(st)} />;
  } else if (tr && tr !== "none") {
    primary = <TrainingStatusIcon status={tr} />;
  } else if (st) {
    primary = <StatusMark kind={markForPackageStatus(st)} />;
  } else if (props.row.hasEditorial) {
    primary = <Icon name="filePenLine" size="sm" tone="secondary-foreground" />;
  } else if (props.row.hasSolutions) {
    primary = <Icon name="code" size="sm" tone="secondary-foreground" />;
  }

  return (
    <View
      tooltip={combinedTip ? tip(combinedTip) : undefined}
      style={{
        width: 16,
        height: 16,
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      {primary}
    </View>
  );
}

/** 40pt single-line problem list row (§8.1): checkbox slot, title, tags, 44pt rating slot, 16pt status slot. */
export function LibraryListRow(props: {
  row: LibraryRow;
  showRatingCol: boolean;
  showStatusCol: boolean;
  trainingEnabled?: boolean;
  selected: boolean;
  anySelected: boolean;
  focused: boolean;
  onRowClick: (id: string, opts: { shift: boolean; mod: boolean; checkbox: boolean }) => void;
  onRowDoubleClick: (id: string) => void;
  onRowContextMenu: (row: LibraryRow, x: number, y: number) => void;
}) {
  const hovered = signal(false);
  const tagText = () => formatRowTags(props.row.tags, 2);

  return (
    <View
      onMouseEnter={() => hovered.set(true)}
      onMouseLeave={() => hovered.set(false)}
      onPress={(e) => {
        const mod = e.modifiers.control || e.modifiers.platform;
        const shift = e.modifiers.shift;
        const isCheckboxSlot = e.position.x <= 36;
        if (e.clickCount >= 2 && !isCheckboxSlot && !mod && !shift) {
          props.onRowDoubleClick(props.row.id);
          return;
        }
        props.onRowClick(props.row.id, { shift, mod, checkbox: isCheckboxSlot });
      }}
      onContextMenu={(e) => props.onRowContextMenu(props.row, e.pagePosition.x, e.pagePosition.y)}
      class={props.selected || props.focused ? `bg-accent ${FOCUS_RING}` : `bg-card hover:bg-muted ${FOCUS_RING}`}
      style={{
        height: ROW.list,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[12],
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        flexShrink: 0,
        ...colorTransition(100),
      }}
    >
      {/* Reserved checkbox slot (§6.3): visible on hover, when selected, or when any item is selected. */}
      <View style={{ width: 24, alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
        <TriCheckbox
          state={props.selected ? "all" : "none"}
          visible={() => hovered() || props.selected || props.anySelected}
        />
      </View>

      {/* Title (TYPE.body, flex width, single-line ellipsis) */}
      <Text
        style={{
          ...TYPE.body,
          flexGrow: 1,
          flexShrink: 1,
          minWidth: 96,
          numberOfLines: 1,
          textOverflow: "ellipsis",
        }}
        class="text-foreground"
      >
        {props.row.title}
      </Text>

      {/* Tags (TYPE.caption, secondary color, max 2 + `+N`, shrinks/truncates first when tight) */}
      <If when={tagText().length > 0}>
        <Text
          style={{
            ...TYPE.caption,
            flexShrink: 3,
            minWidth: 0,
            maxWidth: 132,
            numberOfLines: 1,
            textOverflow: "ellipsis",
          }}
          class="text-muted-foreground"
        >
          {tagText()}
        </Text>
      </If>

      {/* Difficulty badge slot (fixed 44pt, hidden workspace-wide if all problems unrated §6.2) */}
      <If when={props.showRatingCol}>
        <View style={{ width: 44, alignItems: "flex-end", justifyContent: "center", flexShrink: 0 }}>
          {props.row.rating != null ? <RatingBadge rating={props.row.rating} /> : null}
        </View>
      </If>

      {/* Status icon slot (fixed 16pt, hidden workspace-wide if all problems have no status §6.2) */}
      <If when={props.showStatusCol}>
        <LibraryStatusIcon row={props.row} trainingEnabled={props.trainingEnabled} />
      </If>
    </View>
  );
}
