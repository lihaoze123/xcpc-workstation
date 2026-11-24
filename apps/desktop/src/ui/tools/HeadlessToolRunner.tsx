// Headless `index.js` tool runner: dynamically renders a form from `ToolSpec.inputs`
// and displays structured `ToolExecutionResult` blocks through `OutputRenderer`.

import { effect, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { t } from "../i18n";
import { Btn } from "../kit";
import { preview } from "../store";
import { MONO } from "../theme";
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

function FieldEditor(props: {
  field: ToolInputSpec;
  value: unknown;
  onChange: (next: unknown) => void;
}) {
  const kind = props.field.kind;

  if (kind === "boolean") {
    const on = Boolean(props.value);
    return (
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <Text style={{ fontSize: 12.5 }} class="text-foreground">
          {props.field.label}
        </Text>
        <Btn
          size="sm"
          variant={on ? "primary" : "outline"}
          label={on ? t("tools.boolOn") : t("tools.boolOff")}
          onPress={() => props.onChange(!on)}
        />
      </View>
    );
  }

  if (kind === "select" && (props.field.options?.length ?? 0) > 0) {
    const current = String(props.value ?? "");
    return (
      <View style={{ gap: 5 }}>
        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
          {props.field.label}
        </Text>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 5 }}>
          <Each of={props.field.options ?? []} key={(o) => o}>
            {(opt) => (
              <Btn
                size="sm"
                variant={current === opt() ? "primary" : "outline"}
                label={opt()}
                onPress={() => props.onChange(opt())}
              />
            )}
          </Each>
        </View>
      </View>
    );
  }

  const multiline = kind === "textarea";
  return (
    <View style={{ gap: 5 }}>
      <Text style={{ fontSize: 12 }} class="text-muted-foreground">
        {props.field.label}
      </Text>
      <Input
        value={props.value == null ? "" : String(props.value)}
        multiline={multiline}
        autoCapitalize="none"
        autoCorrect={false}
        onChange={(e) => props.onChange(e.value)}
        class="border border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-ring"
        style={{
          borderRadius: 6,
          paddingLeft: 10,
          paddingRight: 10,
          paddingTop: multiline ? 8 : 5,
          paddingBottom: multiline ? 8 : 5,
          minHeight: multiline ? 110 : 32,
          fontSize: 12.5,
          fontFamily: kind === "number" || multiline ? MONO : undefined,
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
  const running = signal(false);
  const result = signal<ToolExecutionResult | undefined>(undefined);
  const errorText = signal<string | undefined>(undefined);
  let runSeq = 0;

  const activeTool = (): ToolSpec | undefined =>
    props.group.tools.find((x) => x.id === selectedToolId()) ?? props.group.tools[0];

  async function execute(tool: ToolSpec, values: Record<string, unknown>) {
    const seq = ++runSeq;
    running.set(true);
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
        return;
      }
      const parsed = parseJsonReport<ToolExecutionResult>(res, { blocks: [] });
      result.set(parsed);
    } catch (err) {
      if (seq !== runSeq) return;
      errorText.set(err instanceof Error ? err.message : String(err));
    } finally {
      if (seq === runSeq) running.set(false);
    }
  }

  effect(() => {
    const tool = activeTool();
    if (!tool) return;
    const defaults = defaultInputsForTool(tool);
    const merged =
      props.initialInputs && typeof props.initialInputs === "object" && !Array.isArray(props.initialInputs)
        ? { ...defaults, ...(props.initialInputs as Record<string, unknown>) }
        : defaults;
    formValues.set(merged);
    void execute(tool, merged);
  });

  const updateField = (name: string, next: unknown) => {
    formValues.update((prev) => ({ ...prev, [name]: next }));
  };

  return (
    <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
      {/* Left parameter panel */}
      <View
        class="bg-card border-r border-border"
        style={{ width: 310, flexShrink: 0, flexDirection: "column" }}
      >
        <If when={props.group.tools.length > 1}>
          <View
            class="border-b border-border"
            style={{ padding: 10, flexDirection: "row", flexWrap: "wrap", gap: 6 }}
          >
            <Each of={props.group.tools} key={(tool) => tool.id}>
              {(tool) => (
                <Btn
                  size="sm"
                  variant={selectedToolId() === tool().id ? "primary" : "outline"}
                  label={tool().name}
                  onPress={() => selectedToolId.set(tool().id)}
                />
              )}
            </Each>
          </View>
        </If>

        <Scroll style={{ flex: 1 }} contentStyle={{ padding: 12, gap: 12 }}>
          <If when={Boolean(activeTool()?.description)}>
            <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
              {activeTool()?.description ?? ""}
            </Text>
          </If>

          <Each of={activeTool()?.inputs ?? []} key={(f) => f.name}>
            {(field) => (
              <FieldEditor
                field={field()}
                value={formValues()[field().name]}
                onChange={(next) => updateField(field().name, next)}
              />
            )}
          </Each>

          <View style={{ paddingTop: 4 }}>
            <Btn
              testID="tool-run-btn"
              variant="primary"
              icon="play"
              label={running() ? t("tools.running") : t("tools.run")}
              disabled={running() || !activeTool()}
              onPress={() => {
                const tool = activeTool();
                if (tool) void execute(tool, formValues());
              }}
              style={{ width: "100%" }}
            />
          </View>
        </Scroll>
      </View>

      {/* Right output panel */}
      <Scroll style={{ flex: 1 }} contentStyle={{ padding: 16, gap: 12 }}>
        <If when={Boolean(errorText())}>
          <View
            class="bg-card border border-destructive/40"
            style={{ padding: 12, borderRadius: 8 }}
          >
            <Text style={{ fontSize: 12.5 }} class="text-destructive">
              {errorText()}
            </Text>
          </View>
        </If>
        <If when={result()}>
          {(res) => (
            <OutputRenderer
              blocks={res().blocks}
              summary={res().summary}
              graphEditorEnabled={props.graphEditorEnabled}
            />
          )}
        </If>
      </Scroll>
    </View>
  );
}
