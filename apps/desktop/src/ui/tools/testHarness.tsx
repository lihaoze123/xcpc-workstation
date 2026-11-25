// Minimal NSH test entry for headless click-level testing of ToolsView and HeadlessToolRunner.

import { mount, signal, View } from "nsh";
import "../theme";
import { startPluginWatch } from "../store";
import { ToolsView } from "../ToolsView";
import { HeadlessToolRunner } from "./HeadlessToolRunner";
import type { PluginToolGroup } from "./types";

const BOOL_DEMO_GROUP: PluginToolGroup = {
  plugin_id: "graph-editor",
  plugin_name: "图论测试表单",
  plugin_description: "",
  has_ui: false,
  is_builtin: true,
  tools: [
    {
      id: "parse-graph",
      name: "解析图",
      inputs: [
        { name: "format", label: "输入格式", kind: "select", default: "edge_list", options: ["edge_list", "parent_array", "adj_list", "coords"] },
        { name: "text", label: "边表", kind: "textarea", default: "1 2\n2 3" },
        { name: "directed", label: "有向图", kind: "boolean", default: true },
      ],
    },
  ],
};

const WATCH_DEMO_GROUP: PluginToolGroup = {
  plugin_id: "complexity-mem",
  plugin_name: "复杂度估算",
  plugin_description: "",
  has_ui: false,
  is_builtin: false,
  tools: [
    {
      id: "estimate",
      name: "估算",
      inputs: [
        { name: "n", label: "规模 n", kind: "number", default: 100000, required: true },
      ],
    },
  ],
};

export default function ToolsTestHarness() {
  const mode = signal<"tools" | "headless-bool" | "headless-watch">("tools");
  return (
    <View style={{ flex: 1, minHeight: 0 }} class="bg-background">
      <View
        testID="harness-switch-bool"
        onPress={() => mode.set("headless-bool")}
        style={{ height: 4, width: 4 }}
      />
      <View
        testID="harness-switch-watch"
        onPress={() => {
          startPluginWatch();
          mode.set("headless-watch");
        }}
        style={{ height: 4, width: 4 }}
      />
      <If when={mode() === "headless-bool"}>
        <HeadlessToolRunner group={BOOL_DEMO_GROUP} graphEditorEnabled={false} />
      </If>
      <If when={mode() === "headless-watch"}>
        <HeadlessToolRunner group={WATCH_DEMO_GROUP} graphEditorEnabled={false} />
      </If>
      <If when={mode() === "tools"}>
        <ToolsView active />
      </If>
    </View>
  );
}

mount(ToolsTestHarness);
