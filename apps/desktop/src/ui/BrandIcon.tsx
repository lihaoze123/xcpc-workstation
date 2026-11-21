// Icon v3 (minimalist) with native GPU-thread fan-out + pencil-landing animation (§6.3, §8.5).
// Timeline (0.75x calm rhythm, 1400ms for the icon itself):
//   0–960ms   plate + 3 draft papers fade in (0–427ms), scale 0.97→1, fan to -20° / -10° / 0°
//   640–1400ms geometric yellow pencil glides in from top-right and settles at final pose
// Final frame is pixel-identical to `assets/brand/icon.svg`.

import {
  Animated,
  cleanup,
  createAnimatedValue,
  Image,
  reduceMotion,
  View,
  type CompositeAnimation,
  type StyleProp,
  type ViewStyle,
} from "nsh";
import { EASE_EXPO_CURVE } from "./motion";
import plateSvg from "./brand/plate.svg";
import paperBackSvg from "./brand/paper-back.svg";
import paperMidSvg from "./brand/paper-mid.svg";
import paperTopSvg from "./brand/paper-top.svg";
import pencilSvg from "./brand/pencil.svg";

export interface BrandIconController {
  /** Replays the 0–1400ms fan-out + pencil-landing animation (snaps immediately under reduced motion). */
  play(): void;
  /** Immediately snaps every layer to its resting final pose (= static icon.svg). */
  snapToFinal(): void;
}

export interface BrandIconProps {
  /** Display size in points (default 144). */
  size?: number;
  /** Plays the entrance animation on mount (default false; true on the Brand page). */
  animated?: boolean;
  /** Clicking the icon replays the animation (e.g. About page easter egg, §6.3). */
  replayOnPress?: boolean;
  /** Imperative controller hook for external replay or fast-forward. */
  controller?: (ctrl: BrandIconController) => void;
  /** Called when the icon animation finishes naturally or is snapped. */
  onFinish?: () => void;
  style?: StyleProp<ViewStyle>;
  testID?: string;
}

/** Paper fan pivot `(565, 680)` on the `1024×1024` master viewBox. */
const PAPER_PIVOT: [string, string] = ["55.18%", "66.41%"];
/** Pencil tip pivot `(578, 635)` on the `1024×1024` master viewBox. */
const PENCIL_PIVOT: [string, string] = ["56.45%", "62.01%"];

export function BrandIcon(props: BrandIconProps) {
  const size = props.size ?? 144;
  const startAnimated = Boolean(props.animated) && !reduceMotion();

  // Pencil entry offset scaled from master 1024 viewBox: (740 - 578, 420 - 635) = (+162, -215).
  const pencilStartX = (162 / 1024) * size;
  const pencilStartY = (-215 / 1024) * size;

  const rootOpacity = createAnimatedValue(startAnimated ? 0 : 1);
  const rootScale = createAnimatedValue(startAnimated ? 0.97 : 1);
  const backRotate = createAnimatedValue(startAnimated ? 0 : -20);
  const midRotate = createAnimatedValue(startAnimated ? 0 : -10);
  const pencilOpacity = createAnimatedValue(startAnimated ? 0 : 1);
  const pencilX = createAnimatedValue(startAnimated ? pencilStartX : 0);
  const pencilY = createAnimatedValue(startAnimated ? pencilStartY : 0);
  const pencilRotate = createAnimatedValue(startAnimated ? 7 : 0);

  let running: CompositeAnimation | undefined;

  function stopRunning() {
    if (running) {
      running.stop();
      running = undefined;
    }
  }

  function snapToFinal() {
    stopRunning();
    rootOpacity.set(1);
    rootScale.set(1);
    backRotate.set(-20);
    midRotate.set(-10);
    pencilOpacity.set(1);
    pencilX.set(0);
    pencilY.set(0);
    pencilRotate.set(0);
    props.onFinish?.();
  }

  function play() {
    stopRunning();
    if (reduceMotion()) {
      snapToFinal();
      return;
    }
    rootOpacity.set(0);
    rootScale.set(0.97);
    backRotate.set(0);
    midRotate.set(0);
    pencilOpacity.set(0);
    pencilX.set(pencilStartX);
    pencilY.set(pencilStartY);
    pencilRotate.set(7);

    const anim = Animated.parallel([
      Animated.timing(rootOpacity, { toValue: 1, duration: 427, easing: EASE_EXPO_CURVE }),
      Animated.timing(rootScale, { toValue: 1, duration: 960, easing: EASE_EXPO_CURVE }),
      Animated.timing(backRotate, { toValue: -20, duration: 960, easing: EASE_EXPO_CURVE }),
      Animated.timing(midRotate, { toValue: -10, duration: 960, easing: EASE_EXPO_CURVE }),
      Animated.sequence([
        Animated.delay(640),
        Animated.timing(pencilOpacity, { toValue: 1, duration: 293, easing: EASE_EXPO_CURVE }),
      ]),
      Animated.sequence([
        Animated.delay(640),
        Animated.parallel([
          Animated.timing(pencilX, { toValue: 0, duration: 760, easing: EASE_EXPO_CURVE }),
          Animated.timing(pencilY, { toValue: 0, duration: 760, easing: EASE_EXPO_CURVE }),
          Animated.timing(pencilRotate, { toValue: 0, duration: 760, easing: EASE_EXPO_CURVE }),
        ]),
      ]),
    ]);

    running = anim;
    void anim.start(({ finished }) => {
      if (running === anim) running = undefined;
      if (finished) props.onFinish?.();
    });
  }

  const ctrl: BrandIconController = { play, snapToFinal };
  props.controller?.(ctrl);

  if (startAnimated) {
    play();
  }

  cleanup(stopRunning);

  const layerStyle: ViewStyle = {
    position: "absolute",
    top: 0,
    left: 0,
    width: size,
    height: size,
    pointerEvents: "none",
  };

  return (
    <View
      testID={props.testID}
      onPress={props.replayOnPress ? () => play() : undefined}
      style={[
        {
          width: size,
          height: size,
          overflow: "visible",
          cursor: props.replayOnPress ? "pointer" : "default",
        },
        props.style,
      ]}
    >
      <View
        style={{
          width: size,
          height: size,
          overflow: "visible",
          opacity: rootOpacity,
          transform: [{ scale: rootScale }],
          pointerEvents: "none",
        }}
      >
        <View style={layerStyle}>
          <Image src={plateSvg} style={{ width: size, height: size }} />
        </View>
        <View
          style={{
            ...layerStyle,
            transformOrigin: PAPER_PIVOT,
            transform: [{ rotate: backRotate }],
          }}
        >
          <Image src={paperBackSvg} style={{ width: size, height: size }} />
        </View>
        <View
          style={{
            ...layerStyle,
            transformOrigin: PAPER_PIVOT,
            transform: [{ rotate: midRotate }],
          }}
        >
          <Image src={paperMidSvg} style={{ width: size, height: size }} />
        </View>
        <View style={layerStyle}>
          <Image src={paperTopSvg} style={{ width: size, height: size }} />
        </View>
        <View
          style={{
            ...layerStyle,
            opacity: pencilOpacity,
            transformOrigin: PENCIL_PIVOT,
            transform: [{ translateX: pencilX }, { translateY: pencilY }, { rotate: pencilRotate }],
          }}
        >
          <Image src={pencilSvg} style={{ width: size, height: size }} />
        </View>
      </View>
    </View>
  );
}
