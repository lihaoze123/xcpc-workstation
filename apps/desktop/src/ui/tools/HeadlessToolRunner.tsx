// Headless `index.js` tool runner: dynamically renders a form from `ToolSpec.inputs`
// with live debounced execution and displays structured `ToolExecutionResult` blocks
// through `OutputRenderer`.

import { cleanup, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Input } from "nsh/base";
import { t } from "../i18n";
import { Segmented } from "../kit";
import { preview } from "../store";
import { cardShadow, MONO } from "../theme";
import { OutputRenderer } from "./OutputRenderer";
import {
  coerceToolInputs,
  defaultInputsForTool,
  parseJsonReport,
  type PluginToolGroup,
  type ToolExecutionResult,
  type ToolInputSpec,
  type ToolSpec,
} from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
function FieldEditor(props: {
  field: ToolInputSpec;
  value: unknown;
  onChange: (next: unknown, immediate?: boolean) => void;
}) {
  const kind = props.field.kind;

  if (kind === "boolean") {
    const isOn = () => Boolean(props.value);
    return (
      <View
        testID={`tool-field-${props.field.name}`}
        onPress={() => props.onChange(!isOn(), true)}
        style={{
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: SPACE[12],
          paddingLeft: SPACE[12],
          paddingRight: SPACE[12],
          height: CONTROL.lg,
          minWidth: 140,
          borderRadius: RADIUS.sm,
          borderWidth: 1,
        }}
        class={isOn() ? "border-border bg-accent" : "border-border bg-background hover:bg-muted"}
      >
        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
          {props.field.label}
        </Text>
        <Text
          testID={`tool-field-${props.field.name}-state`}
          style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
          class={isOn() ? "text-foreground" : "text-muted-foreground"}
        >
          {isOn() ? t("tools.boolOn") : t("tools.boolOff")}
        </Text>
      </View>
    );
  }

  if (kind === "select" && (props.field.options?.length ?? 0) > 0) {
    const options = props.field.options ?? [];
    const current = () => String(props.value ?? "");

    if (options.length <= 5) {
      const items = options.map((opt) => ({
        value: opt,
        label: opt,
        testID: `tool-field-${props.field.name}-${opt}`,
      }));
      const maxLen = options.reduce((m, o) => Math.max(m, o.length), 2);
      const itemWidth = Math.max(64, Math.min(120, maxLen * 9 + 20));
      return (
        <View style={{ gap: SPACE[4], width: "100%" }}>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
            {props.field.label}
          </Text>
          <Segmented
            items={items}
            value={current()}
            itemWidth={itemWidth}
            onChange={(val) => props.onChange(val, true)}
          />
        </View>
      );
    }

    // Wrapping segmented pill track for larger option sets (e.g. 11 C++ data types)
    return (
      <View style={{ gap: SPACE[4], width: "100%" }}>
        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
          {props.field.label}
        </Text>
        <View
          class="bg-muted border border-border"
          style={{
            flexDirection: "row",
            flexWrap: "wrap",
            gap: SPACE[2],
            padding: SPACE[4],
            borderRadius: RADIUS.md,
          }}
        >
          <Each of={options} key={(o) => o}>
            {(opt) => {
              const active = () => current() === opt();
              return (
                <View
                  testID={`tool-field-${props.field.name}-${opt()}`}
                  onPress={() => props.onChange(opt(), true)}
                  class={active() ? "bg-card" : "hover:bg-background/60"}
                  style={{
                    height: CONTROL.sm,
                    paddingLeft: SPACE[8],
                    paddingRight: SPACE[8],
                    borderRadius: RADIUS.xs,
                    alignItems: "center",
                    justifyContent: "center",
                    boxShadow: active() ? cardShadow() : undefined,
                  }}
                >
                  <Text
                    style={{
                      fontSize: FONT_SIZE.caption,
                      fontVariant: ["tabular-nums"],
                    }}
                    class={active() ? "text-foreground" : "text-secondary-foreground"}
                  >
                    {opt()}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>
      </View>
    );
  }

  const multiline = kind === "textarea";
  return (
    <View style={{ gap: SPACE[4], flexGrow: 1, minWidth: multiline ? "100%" : 160 }}>
      <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
        {props.field.label}
      </Text>
      <Input
        testID={`tool-field-${props.field.name}`}
        value={props.value == null ? "" : String(props.value)}
        multiline={multiline}
        autoCapitalize="none"
        autoCorrect={false}
        onChange={(e) => props.onChange(e.value, false)}
        class="border border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-ring"
        style={{
          borderRadius: RADIUS.sm,
          paddingLeft: SPACE[12],
          paddingRight: SPACE[12],
          paddingTop: multiline ? SPACE[8] : SPACE[4],
          paddingBottom: multiline ? SPACE[8] : SPACE[4],
          minHeight: multiline ? 100 : 32,
          fontSize: FONT_SIZE.caption,
          fontFamily: multiline ? MONO : undefined,
          fontVariant: kind === "number" ? ["tabular-nums"] : undefined,
        }}
      />
    </View>
  );
}

export function HeadlessToolRunner(props: {
  group: PluginToolGroup;
  initialToolId?: string;
  initialInputs?: unknown;
  graphEditorEnabled: boolean;
}) {
  const selectedToolId = signal<string>(
    props.initialToolId && props.group.tools.some((x) => x.id === props.initialToolId)
      ? props.initialToolId
      : (props.group.tools[0]?.id ?? ""),
  );
  const formValues = signal<Record<string, unknown>>({});
  const result = signal<ToolExecutionResult | undefined>(undefined);
  const resultSeq = signal(0);
  const errorText = signal<string | undefined>(undefined);
  let runSeq = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  cleanup(() => {
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
  });

  const activeTool = (): ToolSpec | undefined =>
    props.group.tools.find((x) => x.id === selectedToolId()) ?? props.group.tools[0];

  async function execute(tool: ToolSpec, values: Record<string, unknown>) {
    const seq = ++runSeq;
    errorText.set(undefined);
    try {
      const coerced = coerceToolInputs(tool, values);
      const res = await preview.pluginRunTool(
        props.group.plugin_id,
        tool.id,
        JSON.stringify(coerced),
      );
      if (seq !== runSeq) return;
      if (!res.ok) {
        errorText.set(res.error ?? t("tools.runFailed"));
        result.set(undefined);
        resultSeq.update((s) => s + 1);
        return;
      }
      const parsed = parseJsonReport<ToolExecutionResult>(res, { blocks: [] });
      result.set(parsed);
      resultSeq.update((s) => s + 1);
    } catch (err) {
      if (seq !== runSeq) return;
      errorText.set(err instanceof Error ? err.message : String(err));
    }
  }

  function scheduleRun(tool: ToolSpec, values: Record<string, unknown>, immediate: boolean) {
    if (debounceTimer !== undefined) {
      clearTimeout(debounceTimer);
      debounceTimer = undefined;
    }
    if (immediate) {
      void execute(tool, values);
      return;
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      void execute(tool, values);
    }, 120);
  }

  effect(() => {
    const tools = props.group.tools;
    const cur = untracked(() => selectedToolId());
    if (!tools.some((x) => x.id === cur)) {
      const next =
        props.initialToolId && tools.some((x) => x.id === props.initialToolId)
          ? props.initialToolId
          : (tools[0]?.id ?? "");
      selectedToolId.set(next);
    }
  });

  effect(() => {
    const tool = activeTool();
    if (!tool) return;
    const defaults = defaultInputsForTool(tool);
    const merged =
      props.initialInputs && typeof props.initialInputs === "object" && !Array.isArray(props.initialInputs)
        ? { ...defaults, ...(props.initialInputs as Record<string, unknown>) }
        : defaults;
    formValues.set(merged);
    scheduleRun(tool, merged, true);
  });

  const updateField = (name: string, next: unknown, immediate = false) => {
    const nextValues = { ...formValues(), [name]: next };
    formValues.set(nextValues);
    const tool = activeTool();
    if (tool) {
      scheduleRun(tool, nextValues, immediate);
    }
  };

  const toolTabItems = () =>
    props.group.tools.map((tool) => ({
      value: tool.id,
      label: tool.name,
      testID: `tool-tab-${tool.id}`,
    }));

  const toolTabWidth = () => {
    const maxLen = props.group.tools.reduce((m, x) => Math.max(m, x.name.length), 4);
    return Math.max(108, Math.min(156, maxLen * 14 + 24));
  };

  return (
    <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ padding: SPACE[16], gap: SPACE[12] }}>
      {/* Sub-tool switcher via host Segmented */}
      <If when={props.group.tools.length > 1}>
        <View style={{ flexDirection: "row", alignItems: "center" }}>
          <Segmented
            items={toolTabItems()}
            value={selectedToolId()}
            itemWidth={toolTabWidth()}
            onChange={(id) => selectedToolId.set(id)}
          />
        </View>
      </If>

      {/* Stacked parameter form card */}
      <View
        class="bg-card border border-border"
        style={{
          padding: SPACE[12],
          borderRadius: RADIUS.lg,
          gap: SPACE[12],
          boxShadow: cardShadow(),
        }}
      >
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[12], alignItems: "flex-end" }}>
          <Each of={activeTool()?.inputs ?? []} key={(f) => `${selectedToolId()}:${f.name}`}>
            {(field) => (
              <FieldEditor
                field={field()}
                value={formValues()[field().name]}
                onChange={(next, immediate) => updateField(field().name, next, immediate)}
              />
            )}
          </Each>
        </View>
      </View>

      {/* Live output area */}
      <If when={Boolean(errorText())}>
        <View
          class="bg-card border border-destructive/40"
          style={{ padding: SPACE[12], borderRadius: RADIUS.md }}
        >
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-destructive">
            {errorText()}
          </Text>
        </View>
      </If>
      <If when={result()}>
        {(res) => (
          <OutputRenderer
            blocks={res().blocks}
            summary={res().summary}
            resultSeq={resultSeq()}
            graphEditorEnabled={props.graphEditorEnabled}
          />
        )}
      </If>
    </Scroll>
  );
}
