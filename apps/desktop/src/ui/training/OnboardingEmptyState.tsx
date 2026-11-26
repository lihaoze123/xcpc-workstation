// Inline onboarding empty state for Training Overview (1.2):
// Lets the user enter their Codeforces / AtCoder handle directly on the Training page,
// auto-verifying & saving on blur or Enter with a fixed-width right status slot (zero layout shift),
// and immediately triggering sync once verified.

import { signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, DelayedSpinner, tip, TruncText } from "../kit";
import { goSettings, info, notifyPluginsChanged, preview } from "../store";
import { cardShadow } from "../theme";
import { zhEn } from "./locale";
import { platformDisplayName } from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
export interface OnboardingOjSource {
  id: string;
  name: string;
  handle: string;
}

type VerifyState =
  | { kind: "idle" }
  | { kind: "verifying" }
  | { kind: "ok"; message: string }
  | { kind: "error"; message: string };

function HandleInputRow(props: {
  source: OnboardingOjSource;
  onConfigured: () => void;
}) {
  const draft = signal(props.source.handle);
  const state = signal<VerifyState>({ kind: "idle" });
  const lastSubmitted = signal(props.source.handle.trim());
  let lastFailedDraft: string | undefined;

  const saveAndVerify = async (force = false) => {
    const val = draft().trim();
    if (!val) {
      lastFailedDraft = undefined;
      state.set({ kind: "idle" });
      return;
    }
    if (!force && ((val === lastSubmitted() && state().kind === "ok") || val === lastFailedDraft)) return;
    if (state().kind === "verifying") return;

    lastSubmitted.set(val);
    state.set({ kind: "verifying" });

    try {
      const verifyRes = await preview.verifyPlugin(props.source.id, val);
      if (verifyRes.ok) {
        const saveRes = preview.setSyncHandle(props.source.id, val);
        if (!saveRes.ok) {
          lastFailedDraft = val;
          state.set({
            kind: "error",
            message: saveRes.error || zhEn("保存账号失败", "Failed to save handle"),
          });
          return;
        }
        lastFailedDraft = undefined;
        state.set({
          kind: "ok",
          message: verifyRes.message || zhEn("验证通过", "Verified"),
        });
        notifyPluginsChanged();
        props.onConfigured();
      } else {
        lastFailedDraft = val;
        state.set({
          kind: "error",
          message: verifyRes.error || verifyRes.message || zhEn("未找到该账号", "Handle not found"),
        });
      }
    } catch (err) {
      lastFailedDraft = val;
      state.set({
        kind: "error",
        message: err instanceof Error ? err.message : zhEn("验证失败", "Verification failed"),
      });
    }
  };

  const errorMessage = () =>
    state().kind === "error" ? (state() as { kind: "error"; message: string }).message : "";

  return (
    <View style={{ gap: SPACE[2] }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12] }}>
        <Text
          style={{ width: 92, fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, flexShrink: 0 }}
          class="text-foreground"
        >
          {platformDisplayName(props.source.id) || props.source.name}
        </Text>
        <View
          class="bg-background border border-border focus-within:border-input"
          style={{
            flex: 1,
            height: CONTROL.lg,
            borderRadius: RADIUS.sm,
            paddingLeft: SPACE[12],
            paddingRight: SPACE[8],
            flexDirection: "row",
            alignItems: "center",
            gap: SPACE[6],
          }}
        >
          <Input
            testID={`training-onboarding-input-${props.source.id}`}
            class="bg-transparent placeholder:text-muted-foreground"
            style={{ flex: 1, height: CONTROL.md, fontSize: FONT_SIZE.body }}
            value={draft()}
            placeholder={zhEn("输入用户名，回车验证", "Enter handle, press Enter")}
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="blurAndSubmit"
            onChange={(e) => {
              draft.set(e.value);
              lastFailedDraft = undefined;
              if (state().kind !== "idle" && state().kind !== "verifying") {
                state.set({ kind: "idle" });
              }
            }}
            onSubmit={() => void saveAndVerify(true)}
            onBlur={() => void saveAndVerify(false)}
          />
          {/* Fixed-width status slot inside the input: zero layout shift */}
          <View
            testID={`training-onboarding-status-${props.source.id}`}
            style={{ width: 20, height: 20, alignItems: "center", justifyContent: "center", flexShrink: 0 }} // token-exempt: compact inline icon box
            tooltip={
              state().kind === "ok"
                ? tip((state() as { kind: "ok"; message: string }).message)
                : undefined
            }
          >
            <If when={state().kind === "verifying"}>
              <DelayedSpinner active size={12} />
            </If>
            <If when={state().kind === "ok"}>
              <Icon name="check" size="sm" tone="chart-2" />
            </If>
            <If when={state().kind === "error"}>
              <Icon name="circleAlert" size="sm" tone="destructive" />
            </If>
          </View>
        </View>
      </View>
      {/* Fixed single-line body-font error slot directly under the input: zero shift, no monospace */}
      {/* token-exempt: aligned with 104px form label column */}
      <View style={{ marginLeft: 104, height: 16, justifyContent: "center" }}>
        <TruncText
          testID={`training-onboarding-error-${props.source.id}`}
          style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
          class="text-destructive"
        >
          {errorMessage()}
        </TruncText>
      </View>
    </View>
  );
}

export function OnboardingEmptyState(props: {
  sources: readonly OnboardingOjSource[];
  onAccountConfigured: () => void;
  onOpenManualLog?: () => void;
}) {
  const effectiveSources = (): OnboardingOjSource[] => {
    if (props.sources.length > 0) return [...props.sources];
    return [
      { id: "codeforces", name: "Codeforces", handle: "" },
      { id: "atcoder", name: "AtCoder", handle: "" },
    ];
  };

  return (
    <View
      testID="training-overview-empty"
      style={{
        maxWidth: 560,
        width: "100%",
        alignSelf: "center",
        marginTop: SPACE[24],
        gap: SPACE[16],
      }}
    >
      {/* Path A: Manual local training log */}
      <View
        testID="training-onboarding-manual-card"
        class="bg-card border border-border"
        style={{
          borderRadius: RADIUS.lg,
          padding: SPACE[24],
          gap: SPACE[12],
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "flex-start", justifyContent: "space-between", gap: SPACE[16] }}>
          <View style={{ flex: 1, gap: SPACE[4] }}>
            <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("training.onboarding.manualTitle")}
            </Text>
            <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-muted-foreground">
              {t("training.onboarding.manualDesc")}
            </Text>
          </View>
          <Btn
            testID="training-onboarding-manual-log-btn"
            label={t("training.onboarding.manualAction")}
            icon="plus"
            variant="primary"
            disabled={info().readOnly || !props.onOpenManualLog}
            onPress={() => props.onOpenManualLog?.()}
          />
        </View>
      </View>

      {/* Path B: Bind Codeforces / AtCoder accounts */}
      <View
        testID="training-onboarding-oj-card"
        class="bg-card border border-border"
        style={{
          borderRadius: RADIUS.lg,
          padding: SPACE[24],
          gap: SPACE[16],
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ gap: SPACE[4] }}>
          <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
            {t("training.onboarding.ojTitle")}
          </Text>
          <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-muted-foreground">
            {t("training.onboarding.ojDesc")}
          </Text>
        </View>

        <View style={{ gap: SPACE[4] }}>
          <Each of={effectiveSources()} key={(s) => s.id}>
            {(src) => (
              <HandleInputRow
                source={src()}
                onConfigured={props.onAccountConfigured}
              />
            )}
          </Each>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
          <Btn
            testID="training-empty-settings-btn"
            label={zhEn("在设置中管理插件", "Manage in Settings")}
            variant="ghost"
            size="sm"
            onPress={() => goSettings("plugins")}
          />
        </View>
      </View>
    </View>
  );
}
