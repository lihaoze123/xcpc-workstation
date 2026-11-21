// Brand page shown only when no workspace is open (first launch or missing remembered
// workspace; §6.3, §8.5). Plays a calm 1.53s (0.75x) entrance on the native frame clock:
//   0–960ms    Icon plate + 3 draft papers fade in & fan out (-20° / -10° / 0°)
//   640–1400ms Geometric yellow pencil glides in & settles
//   900–1530ms Wordmark, 3 action buttons, and footer stagger in (opacity + translateY)
// Any pointer press, keystroke, or scroll wheel snaps immediately to final; buttons
// remain non-interactive while completely transparent (`opacity === 0`) to prevent blind
// misclicks, and `replayOnly` keeps the 3 workspace entries non-destructive during replay.

import {
  Animated,
  cleanup,
  createAnimatedValue,
  keymap,
  reduceMotion,
  signal,
  Text,
  View,
  type CompositeAnimation,
} from "nsh";
import { BrandIcon, type BrandIconController } from "./BrandIcon";
import { ErrorDisplay } from "./ErrorDisplay";
import { Btn, tip } from "./kit";
import { EASE_EXPO_CURVE } from "./motion";
import { formatAppVersion, readAboutInfo } from "./SettingsView";
import { info, newWorkspace, openError, pickWorkspace } from "./store";

export interface NoWorkspaceViewProps {
  onNew?: () => void;
  onOpen?: () => void;
  onOpenExample?: () => void;
  /** Optional version override (defaults to `formatAppVersion(readAboutInfo())`). */
  version?: string;
  /** When replayed from Settings -> About, display the 3 workspace buttons without switching workspaces. */
  replayOnly?: boolean;
}

const STAGGER_DELAYS = [900, 985, 1070, 1155, 1240] as const;
const STAGGER_DURATION = 290;

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

  // 0: buttons hidden (opacity 0), 1..3: buttons 1..3 have started fading in.
  const visibleStage = signal(shouldAnimate ? 0 : 3);

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
    visibleStage.set(3);
    iconCtrl?.snapToFinal();
    for (let i = 0; i < STAGGER_DELAYS.length; i++) {
      opacities[i].set(1);
      offsets[i].set(0);
    }
  }

  if (shouldAnimate) {
    for (let stage = 1; stage <= 3; stage++) {
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
        visibleStage.set(3);
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
    else void newWorkspace();
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

  const handleOpenExample = () => {
    if (props.replayOnly || visibleStage() < 3) {
      snapToFinal();
      return;
    }
    snapToFinal();
    props.onOpenExample?.();
  };

  const btnStyle = { width: 240, height: 36, borderRadius: 8 };
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
      <View style={{ flex: 1, alignItems: "center", justifyContent: "center", paddingLeft: 24, paddingRight: 24 }}>
        <BrandIcon
          testID="brand-icon"
          size={144}
          animated={shouldAnimate}
          controller={(c) => (iconCtrl = c)}
        />

        <View
          style={{
            marginTop: 20,
            flexDirection: "row",
            alignItems: "baseline",
            justifyContent: "center",
            gap: 7,
            opacity: opacities[0],
            transform: [{ translateY: offsets[0] }],
          }}
        >
          <Text style={{ fontSize: 26, lineHeight: 32, fontWeight: 700, letterSpacing: -0.4 }} class="text-foreground">
            XCPC
          </Text>
          <Text style={{ fontSize: 26, lineHeight: 32, fontWeight: 400, letterSpacing: -0.3 }} class="text-secondary-foreground">
            Workstation
          </Text>
        </View>

        <View style={{ marginTop: 28, width: 240, gap: 8 }}>
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
              label="新建工作区"
              onPress={handleNew}
              style={btnStyle}
              textStyle={{ fontSize: 13.5, fontWeight: 500 }}
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
              label="打开已有文件夹"
              onPress={handleOpen}
              tooltip={props.replayOnly ? undefined : tip("打开已有文件夹", "mod+o")}
              style={btnStyle}
              textStyle={{ fontSize: 13.5, fontWeight: 500 }}
            />
          </View>
          <View
            style={{
              pointerEvents: !props.replayOnly && visibleStage() >= 3 ? "auto" : "none",
              opacity: opacities[3],
              transform: [{ translateY: offsets[3] }],
            }}
          >
            <Btn
              testID="open-example"
              variant="outline"
              size="md"
              label="打开示例"
              onPress={handleOpenExample}
              style={btnStyle}
              textStyle={{ fontSize: 13.5, fontWeight: 500 }}
            />
          </View>
        </View>

        {/* Fixed-height slot for missing-workspace / open errors so layout never shifts (§6.3). */}
        <View style={{ minHeight: 36, width: 320, maxWidth: "90%", marginTop: 8, justifyContent: "center" }}>
          <If
            when={!props.replayOnly && Boolean(openError())}
            else={
              !props.replayOnly && info().lastWorkspaceMissing ? (
                <Text style={{ fontSize: 12, textAlign: "center" }} class="text-secondary-foreground">
                  上次的工作区已不存在
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
          paddingTop: 14,
          paddingBottom: 20,
          paddingLeft: 24,
          paddingRight: 24,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          gap: 16,
          opacity: opacities[4],
          transform: [{ translateY: offsets[4] }],
        }}
      >
        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
          {versionText}
        </Text>
        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
          chumeng 用 ♥ 制作
        </Text>
      </View>
    </View>
  );
}
