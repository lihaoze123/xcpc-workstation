// Two-column distribution cards on the Overview tab:
// Left: Unified Rating bucket bar chart (click any bar to drill down to Submissions).
// Right: Tag distribution with minimum-rating filter (All / >=1600 / >=1900 / >=2200),
//        click any tag to drill down to Submissions with tag + minRating.

import { computed, Text, View } from "nsh";
import { t } from "../i18n";
import { ratingTier } from "../logic";
import { cardShadow, tierColor } from "../theme";
import { isEn, zhEn } from "./locale";
import { formatTagLabel, type DifficultyBucket, type TagStat } from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
const TAG_MIN_OPTIONS: readonly { value: number | null; labelKey: "all" | "1600" | "1900" | "2200" }[] = [
  { value: null, labelKey: "all" },
  { value: 1600, labelKey: "1600" },
  { value: 1900, labelKey: "1900" },
  { value: 2200, labelKey: "2200" },
];

function WindowBadge(props: { label?: string }) {
  return (
    <If when={props.label}>
      <View
        class="bg-muted"
        style={{ height: 18, paddingLeft: SPACE[6], paddingRight: SPACE[6], borderRadius: RADIUS.xs, justifyContent: "center" }} // token-exempt: compact legend badge height
      >
        <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
          {props.label!}
        </Text>
      </View>
    </If>
  );
}

export function DistributionsRow(props: {
  buckets: readonly DifficultyBucket[];
  tags: readonly TagStat[];
  windowLabel?: string;
  tagMinRating: number | null;
  onTagMinRatingChange: (min: number | null) => void;
  onSelectBucket: (bucket: DifficultyBucket) => void;
  onSelectTag: (tag: string, minRating: number | null) => void;
}) {
  const maxBucketAc = computed(() => Math.max(1, ...props.buckets.map((b) => b.ac_count)));
  const visibleTags = computed(() => props.tags.slice(0, 14));
  const maxTagAc = computed(() => Math.max(1, ...visibleTags().map((x) => x.ac_count)));

  return (
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[12] }}>
      {/* Left: Difficulty buckets (restrained cohesive primary opacity ramp, no rainbow wall) */}
      <View
        testID="training-buckets-card"
        class="bg-card border border-border"
        style={{
          flex: 1,
          minWidth: 320,
          borderRadius: RADIUS.lg,
          padding: SPACE[16],
          gap: SPACE[12],
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("training.buckets.title")}
            </Text>
            <WindowBadge label={props.windowLabel} />
          </View>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
            {t("training.buckets.clickHint")}
          </Text>
        </View>

        <View style={{ gap: SPACE[6] }}>
          <Each of={props.buckets} key={(b) => b.label}>
            {(bucket, idx) => {
              const pct = () => Math.max(bucket().ac_count > 0 ? 4 : 0, Math.round((bucket().ac_count / maxBucketAc()) * 100));
              const barOpacity = () => {
                const denom = Math.max(1, props.buckets.length - 1);
                return 0.42 + 0.58 * (idx() / denom);
              };
              return (
                <View
                  testID={`training-bucket-${bucket().label}`}
                  onPress={() => {
                    if (bucket().ac_count > 0) props.onSelectBucket(bucket());
                  }}
                  class={bucket().ac_count > 0 ? "hover:bg-muted/70 rounded-[6px]" : ""}
                  style={{
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[8],
                    paddingLeft: SPACE[6],
                    paddingRight: SPACE[6],
                    height: CONTROL.sm,
                    opacity: bucket().ac_count > 0 ? 1 : 0.45,
                  }}
                >
                  <Text
                    style={{ width: 78, fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
                    class="text-secondary-foreground"
                  >
                    {bucket().label}
                  </Text>
                  <View style={{ flex: 1, height: 10, borderRadius: RADIUS.xs, overflow: "hidden" }} class="bg-muted">
                    <View
                      style={{
                        width: `${pct()}%`,
                        height: 10,
                        borderRadius: RADIUS.xs,
                        opacity: barOpacity(),
                      }}
                      class="bg-primary"
                    />
                  </View>
                  <Text
                    style={{ width: 36, textAlign: "right", fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }}
                    class="text-secondary-foreground"
                  >
                    {String(bucket().ac_count)}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>
      </View>

      {/* Right: High-difficulty tag distribution */}
      <View
        testID="training-tags-card"
        class="bg-card border border-border"
        style={{
          flex: 1,
          minWidth: 320,
          borderRadius: RADIUS.lg,
          padding: SPACE[16],
          gap: SPACE[12],
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: SPACE[8] }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("training.tags.title")}
            </Text>
            <WindowBadge label={props.windowLabel} />
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
            <Each of={TAG_MIN_OPTIONS} key={(o) => o.labelKey}>
              {(opt) => {
                const active = () => props.tagMinRating === opt().value;
                return (
                  <View
                    testID={`training-tag-min-${opt().labelKey}`}
                    onPress={() => props.onTagMinRatingChange(opt().value)}
                    class={active() ? "bg-accent border border-border" : "border border-transparent hover:bg-muted"}
                    style={{ height: CONTROL.sm, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.xs, justifyContent: "center" }}
                  >
                    <Text
                      style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], fontWeight: FONT_WEIGHT.semibold }}
                      class={active() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {opt().value === null ? t("training.tags.filterAll") : `≥${opt().value}`}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </View>

        <If
          when={visibleTags().length > 0}
          else={
            <View style={{ minHeight: 140, alignItems: "center", justifyContent: "center" }}>
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                {t("training.tags.empty")}
              </Text>
            </View>
          }
        >
          <View style={{ gap: SPACE[6] }}>
            {/* Subtle column header row so right colored numbers are immediately understood as average difficulty */}
            <View
              testID="training-tags-header"
              style={{
                flexDirection: "row",
                alignItems: "center",
                gap: SPACE[8],
                paddingLeft: SPACE[6],
                paddingRight: SPACE[6],
                height: 18, // token-exempt: compact chart bar height
              }}
            >
              <Text style={{ width: 112, fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                {zhEn("标签", "Tag")}
              </Text>
              <View style={{ flex: 1 }} />
              <Text style={{ width: 32, textAlign: "right", fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                {zhEn("过题", "Solved")}
              </Text>
              <Text style={{ width: 42, textAlign: "right", fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                {zhEn("均难", "Avg")}
              </Text>
            </View>
            <Each of={visibleTags()} key={(item) => item.tag}>
              {(item) => {
                const pct = () => Math.max(4, Math.round((item().ac_count / maxTagAc()) * 100));
                const avgColor = () => tierColor(ratingTier(item().avg_rating));
                return (
                  <View
                    testID={`training-tag-${item().tag}`}
                    onPress={() => props.onSelectTag(item().tag, props.tagMinRating)}
                    class="hover:bg-muted/70 rounded-[6px]"
                    style={{
                      flexDirection: "row",
                      alignItems: "center",
                      gap: SPACE[8],
                      paddingLeft: SPACE[6],
                      paddingRight: SPACE[6],
                      height: CONTROL.sm,
                    }}
                  >
                    <Text
                      style={{
                        width: 112,
                        fontSize: FONT_SIZE.caption,
                        numberOfLines: 1,
                        textOverflow: "ellipsis",
                      }}
                      class="text-foreground"
                    >
                      {formatTagLabel(item().tag, isEn())}
                    </Text>
                    <View style={{ flex: 1, height: 10, borderRadius: RADIUS.xs, overflow: "hidden" }} class="bg-muted">
                      <View
                        style={{
                          width: `${pct()}%`,
                          height: 10,
                          borderRadius: RADIUS.xs,
                        }}
                        class="bg-primary"
                      />
                    </View>
                    <Text
                      style={{ width: 32, textAlign: "right", fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }}
                      class="text-secondary-foreground"
                    >
                      {String(item().ac_count)}
                    </Text>
                    <Text
                      style={{
                        width: 42,
                        textAlign: "right",
                        fontSize: FONT_SIZE.caption,
                        fontVariant: ["tabular-nums"],
                        color: item().avg_rating != null ? avgColor() : undefined,
                      }}
                      class={item().avg_rating != null ? undefined : "text-muted-foreground"}
                    >
                      {item().avg_rating != null ? String(item().avg_rating) : ""}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </If>
      </View>
    </View>
  );
}
