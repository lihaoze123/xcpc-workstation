// Headless harness for `truncText.test.tsx`: three single-line labels and one two-line label
// in fixed-width rows, so the test can hover each and see whether a tooltip appears.

import "./theme";
import { mount, View } from "nsh";
import { TruncText } from "./kit";
import { FONT_SIZE, SPACE } from "./tokens";

const LONG_EN =
  "A very long problem description used to check that a two-line clamp shows the full text in a tooltip on hover";
const LONG = "一道题名非常非常长的题目用来测试截断后悬停是否显示完整标题的提示框是否正常工作";

function App() {
  return (
    <View style={{ padding: SPACE[16], gap: SPACE[8], width: 400 }}>
      <View style={{ width: 160, flexDirection: "row" }}>
        <TruncText testID="cut" style={{ flex: 1, minWidth: 0, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis" }}>
          {LONG}
        </TruncText>
      </View>
      <View style={{ width: 160, flexDirection: "row" }}>
        <TruncText testID="fits" style={{ flex: 1, minWidth: 0, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis" }}>
          短题名
        </TruncText>
      </View>
      <View style={{ width: 160, flexDirection: "row" }}>
        <TruncText testID="hug" style={{ flexShrink: 1, minWidth: 0, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis" }}>
          短题名
        </TruncText>
      </View>
      <View style={{ width: 160 }}>
        <TruncText testID="cut2" style={{ fontSize: FONT_SIZE.body, numberOfLines: 2, textOverflow: "ellipsis" }}>
          {LONG_EN}
        </TruncText>
      </View>
    </View>
  );
}

mount(App);
