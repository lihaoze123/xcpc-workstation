// Brand page shown only when no workspace is open (first launch or missing remembered
// workspace; §6.3, §8.5). Plays a calm 1.53s (0.75x) entrance on the native frame clock:
//   0–960ms    Icon plate + 3 draft papers fade in & fan out (-20° / -10° / 0°)
//   640–1400ms Geometric yellow pencil glides in & settles
//   900–1530ms Wordmark, 2 action buttons, status notice, and footer stagger in (opacity + translateY)
// Any pointer press, keystroke, or scroll wheel snaps immediately to final; buttons
// remain non-interactive while completely transparent (`opacity === 0`) to prevent blind
// misclicks, and `replayOnly` keeps the workspace entries non-destructive during replay.

import {
  Animated,
  cleanup,
  createAnimatedValue,
  Easing,
  keymap,
  reduceMotion,
  signal,
  Text,
  View,
  type CompositeAnimation,
} from "nsh";
import { BrandIcon, type BrandIconController } from "./BrandIcon";
import { ErrorDisplay } from "./ErrorDisplay";
import { t } from "./i18n";
import { Btn, tip } from "./kit";

// Documented exception (§8.5): brand intro stagger uses an exponential ease-out curve.
const EASE_EXPO_CURVE = Easing.bezier(0.16, 1, 0.3, 1);
import { formatAppVersion, readAboutInfo } from "./SettingsView";
import { goSpace, info, openError, pickWorkspace, requestNewWorkspace } from "./store";

import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, ROW, SPACE } from "./tokens";
export interface NoWorkspaceViewProps {
  onNew?: () => void;
  onOpen?: () => void;
  /** Optional version override (defaults to `formatAppVersion(readAboutInfo())`). */
  version?: string;
  /** When replayed from Settings -> About, display the workspace buttons without switching workspaces. */
  replayOnly?: boolean;
}

const STAGGER_DELAYS = [900, 1005, 1110, 1215] as const;
const STAGGER_DURATION = 315;

const FAST_FORWARD_KEYS: readonly string[] = [
  "space",
  "enter",
  "escape",
  "tab",
  "backspace",
  "delete",
  "up",
  "down",
  "left",
  "right",
  "mod+o",
  "mod+n",
  ..."abcdefghijklmnopqrstuvwxyz0123456789".split(""),
];

export function NoWorkspaceView(props: NoWorkspaceViewProps) {
  const shouldAnimate = !reduceMotion();
  let iconCtrl: BrandIconController | undefined;
  let running: CompositeAnimation | undefined;
  let animating = shouldAnimate;
  let unbindKeys: (() => void) | undefined;
  const stageTimers: number[] = [];

  // 0: buttons hidden (opacity 0), 1..2: buttons 1..2 have started fading in.
  const visibleStage = signal(shouldAnimate ? 0 : 2);

  const opacities = STAGGER_DELAYS.map(() => createAnimatedValue(shouldAnimate ? 0 : 1));
  const offsets = STAGGER_DELAYS.map(() => createAnimatedValue(shouldAnimate ? 6 : 0));

  function clearStageTimers() {
    while (stageTimers.length > 0) {
      clearTimeout(stageTimers.pop());
    }
  }

  function releaseKeymap() {
    if (unbindKeys) {
      unbindKeys();
      unbindKeys = undefined;
    }
  }

  function stopUi() {
    clearStageTimers();
    releaseKeymap();
    if (running) {
      running.stop();
      running = undefined;
    }
  }

  function snapToFinal() {
    if (!animating) return;
    animating = false;
    stopUi();
    visibleStage.set(2);
    iconCtrl?.snapToFinal();
    for (let i = 0; i < STAGGER_DELAYS.length; i++) {
      opacities[i].set(1);
      offsets[i].set(0);
    }
  }

  if (shouldAnimate) {
    for (let stage = 1; stage <= 2; stage++) {
      const s = stage;
      stageTimers.push(
        setTimeout(() => {
          if (visibleStage() < s) visibleStage.set(s);
        }, STAGGER_DELAYS[s]),
      );
    }

    const anim = Animated.parallel(
      STAGGER_DELAYS.map((delay, i) =>
        Animated.sequence([
          Animated.delay(delay),
          Animated.parallel([
            Animated.timing(opacities[i], { toValue: 1, duration: STAGGER_DURATION, easing: EASE_EXPO_CURVE }),
            Animated.timing(offsets[i], { toValue: 0, duration: STAGGER_DURATION, easing: EASE_EXPO_CURVE }),
          ]),
        ]),
      ),
    );
    running = anim;
    void anim.start(({ finished }) => {
      if (running === anim) running = undefined;
      if (finished) {
        animating = false;
        visibleStage.set(2);
        releaseKeymap();
      }
    });

    // Any keystroke fast-forwards to final pose while passing the key straight through.
    const bindings: Record<string, () => void> = {};
    for (const key of FAST_FORWARD_KEYS) bindings[key] = () => snapToFinal();
    unbindKeys = keymap.bind(bindings, { when: "always", passThrough: true });
  }

  cleanup(stopUi);

  const handleNew = () => {
    if (props.replayOnly || visibleStage() < 1) {
      snapToFinal();
      return;
    }
    snapToFinal();
    if (props.onNew) props.onNew();
    else requestNewWorkspace();
  };

  const handleOpen = () => {
    if (props.replayOnly || visibleStage() < 2) {
      snapToFinal();
      return;
    }
    snapToFinal();
    if (props.onOpen) props.onOpen();
    else void pickWorkspace();
  };

  const btnStyle = { width: 240, height: ROW.table, borderRadius: RADIUS.md };
  const versionText = props.version ?? formatAppVersion(readAboutInfo());

  return (
    <View
      testID="brand-page"
      focusable
      ref={(el) => {
        if (el) queueMicrotask(() => el.focus());
      }}
      onPress={() => snapToFinal()}
      onKeyDown={() => snapToFinal()}
      onWheel={() => snapToFinal()}
      style={{ flex: 1, flexDirection: "column" }}
      class="bg-background"
    >
      <View style={{ flex: 1, alignItems: "center", justifyContent: "center", paddingLeft: SPACE[24], paddingRight: SPACE[24] }}>
        <BrandIcon
          testID="brand-icon"
          size={144}
          animated={shouldAnimate}
          controller={(c) => (iconCtrl = c)}
        />

        <View
          style={{
            marginTop: SPACE[20],
            flexDirection: "row",
            alignItems: "baseline",
            justifyContent: "center",
            gap: SPACE[8],
            opacity: opacities[0],
            transform: [{ translateY: offsets[0] }],
          }}
        >
          <Text style={{ fontSize: FONT_SIZE.display, lineHeight: LINE_HEIGHT.display, fontWeight: FONT_WEIGHT.semibold, letterSpacing: -0.4 }} class="text-foreground">
            XCPC
          </Text>
          <Text style={{ fontSize: FONT_SIZE.display, lineHeight: LINE_HEIGHT.display, fontWeight: FONT_WEIGHT.regular, letterSpacing: -0.3 }} class="text-secondary-foreground">
            Workstation
          </Text>
        </View>

        <View style={{ marginTop: SPACE[32], width: 240, gap: SPACE[8] }}>
          <View
            style={{
              pointerEvents: !props.replayOnly && visibleStage() >= 1 ? "auto" : "none",
              opacity: opacities[1],
              transform: [{ translateY: offsets[1] }],
            }}
          >
            <Btn
              testID="new-workspace"
              variant="primary"
              size="md"
              label={t("brand.newWorkspace")}
              onPress={handleNew}
              style={btnStyle}
              textStyle={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }}
            />
          </View>
          <View
            style={{
              pointerEvents: !props.replayOnly && visibleStage() >= 2 ? "auto" : "none",
              opacity: opacities[2],
              transform: [{ translateY: offsets[2] }],
            }}
          >
            <Btn
              testID="open-workspace"
              variant="outline"
              size="md"
              label={t("brand.openExistingFolder")}
              onPress={handleOpen}
              tooltip={props.replayOnly ? undefined : tip(t("brand.openExistingFolder"), "mod+o")}
              style={btnStyle}
              textStyle={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }}
            />
          </View>
          <If when={!props.replayOnly}>
            <View
              style={{
                marginTop: SPACE[6],
                flexDirection: "row",
                justifyContent: "center",
                gap: SPACE[8],
                pointerEvents: visibleStage() >= 2 ? "auto" : "none",
                opacity: opacities[3],
                transform: [{ translateY: offsets[3] }],
              }}
            >
              <Btn
                testID="no-workspace-tools"
                variant="ghost"
                size="sm"
                icon="squareCode"
                label={t("space.tools")}
                tooltip={tip(t("space.tools"), "mod+4")}
                onPress={() => {
                  snapToFinal();
                  goSpace("tools");
                }}
              />
              <Btn
                testID="no-workspace-settings"
                variant="ghost"
                size="sm"
                icon="settings"
                label={t("space.settings")}
                tooltip={tip(t("space.settings"), "mod+5")}
                onPress={() => {
                  snapToFinal();
                  goSpace("settings");
                }}
              />
            </View>
          </If>
        </View>

        {/* Fixed-height slot for missing-workspace / open errors, staggered with the final phase
            so it never floats alone over the blank page during the icon reveal (§6.3). */}
        <View
          style={{
            minHeight: 36,
            width: 320,
            maxWidth: "90%",
            marginTop: SPACE[8],
            justifyContent: "center",
            opacity: opacities[3],
            transform: [{ translateY: offsets[3] }],
          }}
        >
          <If
            when={!props.replayOnly && Boolean(openError())}
            else={
              !props.replayOnly && info().lastWorkspaceMissing ? (
                <Text style={{ fontSize: FONT_SIZE.caption, textAlign: "center" }} class="text-secondary-foreground">
                  {t("brand.lastWorkspaceMissing")}
                </Text>
              ) : null
            }
          >
            <ErrorDisplay error={openError()} align="center" onDismiss={() => openError.set(undefined)} />
          </If>
        </View>
      </View>

      <View
        style={{
          paddingTop: SPACE[12],
          paddingBottom: SPACE[20],
          paddingLeft: SPACE[24],
          paddingRight: SPACE[24],
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          gap: SPACE[16],
          opacity: opacities[3],
          transform: [{ translateY: offsets[3] }],
        }}
      >
        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
          {versionText}
        </Text>
        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
          {t("brand.madeWithLove")}
        </Text>
      </View>
    </View>
  );
}
