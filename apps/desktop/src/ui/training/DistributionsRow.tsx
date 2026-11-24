// Two-column distribution cards on the Overview tab:
// Left: Unified Rating bucket bar chart (click any bar to drill down to Submissions).
// Right: Tag distribution with minimum-rating filter (All / >=1600 / >=1900 / >=2200),
//        click any tag to drill down to Submissions with tag + minRating.

import { computed, Text, View } from "nsh";
import { t } from "../i18n";
import { ratingTier } from "../logic";
import { cardShadow, tierColor } from "../theme";
import type { DifficultyBucket, TagStat } from "./types";

const TAG_MIN_OPTIONS: readonly { value: number | null; labelKey: "all" | "1600" | "1900" | "2200" }[] = [
  { value: null, labelKey: "all" },
  { value: 1600, labelKey: "1600" },
  { value: 1900, labelKey: "1900" },
  { value: 2200, labelKey: "2200" },
];

export function DistributionsRow(props: {
  buckets: readonly DifficultyBucket[];
  tags: readonly TagStat[];
  tagMinRating: number | null;
  onTagMinRatingChange: (min: number | null) => void;
  onSelectBucket: (bucket: DifficultyBucket) => void;
  onSelectTag: (tag: string, minRating: number | null) => void;
}) {
  const maxBucketAc = computed(() => Math.max(1, ...props.buckets.map((b) => b.ac_count)));
  const visibleTags = computed(() => props.tags.slice(0, 14));
  const maxTagAc = computed(() => Math.max(1, ...visibleTags().map((x) => x.ac_count)));

  return (
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 14 }}>
      {/* Left: Difficulty buckets */}
      <View
        testID="training-buckets-card"
        class="bg-card border border-border"
        style={{
          flex: 1,
          minWidth: 320,
          borderRadius: 10,
          padding: 16,
          gap: 12,
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "baseline", justifyContent: "space-between", gap: 8 }}>
          <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
            {t("training.buckets.title")}
          </Text>
          <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
            {t("training.buckets.clickHint")}
          </Text>
        </View>

        <View style={{ gap: 6 }}>
          <Each of={props.buckets} key={(b) => b.label}>
            {(bucket) => {
              const repRating = () => bucket().min_rating ?? (bucket().max_rating != null ? bucket().max_rating! - 100 : 1000);
              const color = () => tierColor(ratingTier(repRating()));
              const pct = () => Math.max(bucket().ac_count > 0 ? 4 : 0, Math.round((bucket().ac_count / maxBucketAc()) * 100));
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
                    gap: 10,
                    paddingLeft: 6,
                    paddingRight: 6,
                    height: 24,
                    opacity: bucket().ac_count > 0 ? 1 : 0.45,
                  }}
                >
                  <Text
                    style={{ width: 78, fontSize: 11.5, fontWeight: 500, fontVariant: ["tabular-nums"], color: color() }}
                  >
                    {bucket().label}
                  </Text>
                  <View style={{ flex: 1, height: 10, borderRadius: 4, overflow: "hidden" }} class="bg-muted">
                    <View
                      style={{
                        width: `${pct()}%`,
                        height: 10,
                        borderRadius: 4,
                        backgroundColor: color(),
                      }}
                    />
                  </View>
                  <Text
                    style={{ width: 36, textAlign: "right", fontSize: 12, fontVariant: ["tabular-nums"] }}
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
          borderRadius: 10,
          padding: 16,
          gap: 12,
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 8 }}>
          <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
            {t("training.tags.title")}
          </Text>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Each of={TAG_MIN_OPTIONS} key={(o) => o.labelKey}>
              {(opt) => {
                const active = () => props.tagMinRating === opt().value;
                return (
                  <View
                    testID={`training-tag-min-${opt().labelKey}`}
                    onPress={() => props.onTagMinRatingChange(opt().value)}
                    class={active() ? "bg-accent border border-border" : "hover:bg-muted"}
                    style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center" }}
                  >
                    <Text
                      style={{ fontSize: 11.5, fontVariant: ["tabular-nums"], fontWeight: active() ? 600 : 400 }}
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
              <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                {t("training.tags.empty")}
              </Text>
            </View>
          }
        >
          <View style={{ gap: 6 }}>
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
                      gap: 10,
                      paddingLeft: 6,
                      paddingRight: 6,
                      height: 24,
                    }}
                  >
                    <Text
                      style={{
                        width: 112,
                        fontSize: 12,
                        numberOfLines: 1,
                        textOverflow: "ellipsis",
                      }}
                      class="text-foreground"
                    >
                      {item().tag}
                    </Text>
                    <View style={{ flex: 1, height: 10, borderRadius: 4, overflow: "hidden" }} class="bg-muted">
                      <View
                        style={{
                          width: `${pct()}%`,
                          height: 10,
                          borderRadius: 4,
                        }}
                        class="bg-primary"
                      />
                    </View>
                    <Text
                      style={{ width: 32, textAlign: "right", fontSize: 12, fontVariant: ["tabular-nums"] }}
                      class="text-secondary-foreground"
                    >
                      {String(item().ac_count)}
                    </Text>
                    <Text
                      style={{
                        width: 42,
                        textAlign: "right",
                        fontSize: 11.5,
                        fontVariant: ["tabular-nums"],
                        color: item().avg_rating != null ? avgColor() : undefined,
                      }}
                      class={item().avg_rating != null ? undefined : "text-muted-foreground"}
                    >
                      {item().avg_rating != null ? String(item().avg_rating) : "—"}
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
