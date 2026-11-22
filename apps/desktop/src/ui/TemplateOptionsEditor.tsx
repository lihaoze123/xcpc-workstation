// Reusable declarative template UI components:
// - `TemplatePicker`: selects workspace `templates/*.typ` with optional inherit/restore and "add from preset".
// - `TemplateOptionsEditor`: dynamically renders all 9 schema field types (`bool`, `enum`, `string`,
//   `lines`, `number`, `length`, `color`, `font`, `image`), grouped with `when` conditional visibility,
//   override indicator bars, and one-click restore.
// - `ForkTemplateModal`: forks an existing template into `templates/<newName>.typ`.
// - `InstallPresetModal`: installs a bundled preset seed into `templates/<targetName>.typ`.

import { cleanup, computed, effect, signal, Text, untracked, View } from "nsh";
import { Input, Switch } from "nsh/base";
import { ErrorDisplay } from "./ErrorDisplay";
import { Icon } from "./Icon";
import { Btn, Chevron, IconBtn, Segmented, tip } from "./kit";
import { pathBase, validId } from "./logic";
import { AppMenu, type MenuEntry } from "./Menu";
import { Modal } from "./Modal";
import { colorTransition } from "./motion";
import { errorOf, preview, type UiError } from "./store";
import {
  isFieldVisible,
  optionValuesEqual,
  type PresetInfo,
  type TemplateField,
  type TemplateFieldScope,
  type TemplateInfo,
  type TemplateOptionMap,
  type TemplateOptionValue,
} from "./templates";
import { MONO } from "./theme";

const ADD_PRESET_ID = "__add_preset__";

/** Dropdown selector for choosing a workspace template (`templates/*.typ`). */
export function TemplatePicker(props: {
  templates: TemplateInfo[];
  value: string;
  inheritedLabel?: string;
  overridden?: boolean;
  readOnly?: boolean;
  onChange: (templateId: string) => void;
  onRestore?: () => void;
  onAddFromPreset?: () => void;
}) {
  const current = computed(() => props.templates.find((t) => t.id === props.value));
  const entries = computed<MenuEntry[]>(() => {
    const list: MenuEntry[] = props.templates.map((t) => ({
      id: t.id,
      label: t.name,
      checked: t.id === props.value,
    }));
    if (props.onAddFromPreset) {
      if (list.length > 0) list.push({ separator: true, id: "sep-add-preset" });
      list.push({
        id: ADD_PRESET_ID,
        label: "从预设添加模板…",
        icon: "plus",
      });
    }
    return list;
  });

  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
        <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
          模板
        </Text>
        <If when={props.overridden && !props.readOnly && !!props.onRestore}>
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.inheritedLabel ?? "还原为工作区默认模板")} onPress={() => props.onRestore?.()} />
        </If>
      </View>
      <View style={{ flexDirection: "row" }}>
        <View
          style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
          class="bg-primary"
        />
        <View style={{ flex: 1 }}>
          <If
            when={!props.readOnly}
            else={
              <View
                class="border border-border bg-muted/50"
                style={{
                  height: 32,
                  borderRadius: 6,
                  paddingLeft: 10,
                  paddingRight: 8,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 6,
                }}
              >
                <Text style={{ flex: 1, fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-secondary-foreground">
                  {current()?.name ?? props.value}
                </Text>
              </View>
            }
          >
            <AppMenu
              entries={entries()}
              onSelect={(id) => {
                if (id === ADD_PRESET_ID) {
                  props.onAddFromPreset?.();
                } else if (id !== props.value) {
                  props.onChange(id);
                }
              }}
              minWidth={220}
              trigger={
                <View
                  testID="template-picker"
                  class="border border-border bg-card hover:bg-muted active:bg-accent"
                  style={{
                    height: 32,
                    borderRadius: 6,
                    paddingLeft: 10,
                    paddingRight: 8,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 6,
                    ...colorTransition(120),
                  }}
                >
                  <Text style={{ flex: 1, fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                    {current()?.name ?? props.value}
                  </Text>
                  <Chevron size={13} tone="muted-foreground" />
                </View>
              }
            />
          </If>
        </View>
      </View>
      <If when={!!current()?.description}>
        <Text style={{ fontSize: 11.5, lineHeight: 16 }} class="text-muted-foreground">
          {current()!.description}
        </Text>
      </If>
    </View>
  );
}

/** Renders all schema fields for `scope` (`"book"` or `"problem"`), grouped and filtered by `when`. */
export function TemplateOptionsEditor(props: {
  ownerKey: string;
  template: TemplateInfo | undefined;
  scope: TemplateFieldScope;
  /** Inherited values from parent scope (e.g. template defaults merged with workspace defaults). */
  baseValues: TemplateOptionMap;
  /** Explicit overrides set at this scope (keys present here show the override bar + restore button). */
  overrideValues: TemplateOptionMap;
  /** General book switches (`cover`, `language`, `blank-pages`, `show-metadata`) for `when` evaluation. */
  context?: Record<string, TemplateOptionValue>;
  /** Engine-loaded font families for `font` fields. */
  fonts: string[];
  /** Optional contest ID when editing contest-level options (for importing `image` fields into `contests/<id>/assets/`). */
  contestId?: string;
  readOnly?: boolean;
  restoreTooltip?: string;
  /** When true, each schema group renders as its own full-width bordered section (for Inspector). */
  sectioned?: boolean;
  /** When `sectioned` is true, whether the last group omits its bottom border. */
  lastSection?: boolean;
  /** Suppresses group sub-headings when the caller already wraps a single scope in its own titled Section. */
  hideGroupLabels?: boolean;
  onChange: (key: string, value: TemplateOptionValue | null) => void;
}) {
  const scopedFields = computed(() =>
    (props.template?.schema.fields ?? []).filter((f) => f.scope === props.scope),
  );

  // Compute effective resolved value for every field in this scope.
  const resolved = computed<TemplateOptionMap>(() => {
    const out: TemplateOptionMap = {};
    for (const f of scopedFields()) {
      if (f.key in props.overrideValues) {
        out[f.key] = props.overrideValues[f.key];
      } else if (f.key in props.baseValues) {
        out[f.key] = props.baseValues[f.key];
      } else {
        out[f.key] = f.default;
      }
    }
    return out;
  });

  const visibleFields = computed(() =>
    scopedFields().filter((f) => isFieldVisible(f, resolved(), props.context)),
  );

  // Group visible fields according to `schema.groups`, preserving order.
  const grouped = computed<{ id: string; label?: string; fields: TemplateField[] }[]>(() => {
    const list = visibleFields();
    if (list.length === 0) return [];
    const groups = props.template?.schema.groups ?? [];
    const out: { id: string; label?: string; fields: TemplateField[] }[] = [];
    const used = new Set<string>();
    for (const g of groups) {
      const matching = list.filter((f) => f.group === g.id);
      if (matching.length > 0) {
        out.push({ id: g.id, label: g.label, fields: matching });
        for (const f of matching) used.add(f.key);
      }
    }
    const rest = list.filter((f) => !used.has(f.key));
    if (rest.length > 0) {
      out.push({ id: "__ungrouped__", fields: rest });
    }
    return out;
  });

  const baseFor = (field: TemplateField): TemplateOptionValue =>
    field.key in props.baseValues ? props.baseValues[field.key] : field.default;

  const isOverridden = (field: TemplateField): boolean =>
    field.key in props.overrideValues && !optionValuesEqual(props.overrideValues[field.key], baseFor(field));

  const commitValue = (field: TemplateField, next: TemplateOptionValue) => {
    if (optionValuesEqual(next, baseFor(field))) {
      props.onChange(field.key, null);
    } else {
      props.onChange(field.key, next);
    }
  };

  return (
    <View style={props.sectioned ? undefined : { gap: 18 }}>
      <Each of={grouped()} key={(g) => g.id}>
        {(group, idx) => {
          const isLast = () => idx() === grouped().length - 1;
          return (
            <View
              style={
                props.sectioned
                  ? {
                      padding: 14,
                      gap: 10,
                      borderBottomWidth: props.lastSection && isLast() ? 0 : 1,
                    }
                  : {
                      gap: 10,
                      paddingTop: idx() > 0 ? 14 : 0,
                      borderTopWidth: idx() > 0 ? 1 : 0,
                    }
              }
              class="border-border"
            >
              <If when={!props.hideGroupLabels && !!group().label}>
                <Text
                  style={{ fontSize: 12.5, fontWeight: 600, lineHeight: 17 }}
                  class="text-foreground"
                >
                  {group().label}
                </Text>
              </If>
              <Each of={group().fields} key={(f) => `${props.template?.id ?? ""}:${f.key}`}>
                {(field) => (
                  <SchemaFieldRow
                    ownerKey={`${props.ownerKey}:${props.template?.id ?? ""}:${field().key}`}
                    field={field()}
                    value={resolved()[field().key] ?? field().default}
                    overridden={isOverridden(field())}
                    readOnly={props.readOnly}
                    restoreTooltip={props.restoreTooltip ?? "恢复默认值"}
                    fonts={props.fonts}
                    contestId={props.contestId}
                    onCommit={(v) => commitValue(field(), v)}
                    onRestore={() => props.onChange(field().key, null)}
                  />
                )}
              </Each>
            </View>
          );
        }}
      </Each>
    </View>
  );
}

function SchemaFieldRow(props: {
  ownerKey: string;
  field: TemplateField;
  value: TemplateOptionValue;
  overridden: boolean;
  readOnly?: boolean;
  restoreTooltip: string;
  fonts: string[];
  contestId?: string;
  onCommit: (v: TemplateOptionValue) => void;
  onRestore: () => void;
}) {
  const f = () => props.field;
  const ty = () => f().type;

  if (ty() === "bool") {
    const checked = () => Boolean(props.value);
    return (
      <View style={{ gap: 2 }}>
        <View style={{ flexDirection: "row", alignItems: "center", height: 24, gap: 6 }}>
          <View
            style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
            class="bg-primary"
          />
          <Text style={{ flex: 1, fontSize: 13 }} class={props.readOnly ? "text-secondary-foreground" : "text-foreground"}>
            {f().label}
          </Text>
          <If when={props.overridden && !props.readOnly}>
            <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
          </If>
          <Switch
            checked={checked()}
            disabled={props.readOnly}
            onChange={(e) => !props.readOnly && props.onCommit(e.checked)}
            trackClass={
              props.readOnly
                ? "w-8 h-[18px] rounded-full bg-input checked:bg-primary opacity-50"
                : "w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
            }
            thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
          />
        </View>
        <If when={!!f().description}>
          <Text style={{ fontSize: 11.5, lineHeight: 15 }} class="text-muted-foreground">
            {f().description}
          </Text>
        </If>
      </View>
    );
  }

  if (ty() === "enum") {
    const choices = () => f().choices ?? [];
    const strVal = () => String(props.value ?? "");
    const useSegmented = () => choices().length >= 2 && choices().length <= 3 && choices().every((c) => c.label.length <= 5);
    const currentLabel = () => choices().find((c) => c.value === strVal())?.label ?? strVal();

    return (
      <View style={{ gap: 4 }}>
        <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
          <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
            {f().label}
          </Text>
          <If when={props.overridden && !props.readOnly}>
            <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
          </If>
        </View>
        <View style={{ flexDirection: "row" }}>
          <View
            style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
            class="bg-primary"
          />
          <If
            when={useSegmented()}
            else={
              <View style={{ flex: 1 }}>
                <If
                  when={!props.readOnly}
                  else={
                    <View class="border border-border bg-muted/50" style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 8, justifyContent: "center" }}>
                      <Text style={{ fontSize: 13 }} class="text-secondary-foreground">{currentLabel()}</Text>
                    </View>
                  }
                >
                  <AppMenu
                    entries={choices().map((c) => ({ id: c.value, label: c.label, checked: c.value === strVal() }))}
                    onSelect={(v) => props.onCommit(v)}
                    minWidth={180}
                    trigger={
                      <View
                        class="border border-border bg-card hover:bg-muted active:bg-accent"
                        style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 8, flexDirection: "row", alignItems: "center", gap: 6, ...colorTransition(120) }}
                      >
                        <Text style={{ flex: 1, fontSize: 13 }} class="text-foreground">{currentLabel()}</Text>
                        <Chevron size={13} tone="muted-foreground" />
                      </View>
                    }
                  />
                </If>
              </View>
            }
          >
            <Segmented
              disabled={props.readOnly}
              items={choices()}
              itemWidth={choices().length === 2 ? 84 : 68}
              value={strVal()}
              onChange={(v) => !props.readOnly && props.onCommit(v)}
            />
          </If>
        </View>
      </View>
    );
  }

  if (ty() === "font") {
    const strVal = () => String(props.value ?? "");
    const fontEntries = computed<MenuEntry[]>(() => {
      const list: MenuEntry[] = [{ id: "__default__", label: f().placeholder ?? "默认字体", checked: strVal() === "" }];
      const unique = props.fonts.slice(0, 60);
      if (unique.length > 0) {
        list.push({ separator: true, id: "sep-fonts" });
        for (const fam of unique) {
          list.push({ id: fam, label: fam, checked: fam === strVal() });
        }
      }
      return list;
    });
    return (
      <View style={{ gap: 4 }}>
        <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
          <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
            {f().label}
          </Text>
          <If when={props.overridden && !props.readOnly}>
            <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
          </If>
        </View>
        <View style={{ flexDirection: "row" }}>
          <View
            style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
            class="bg-primary"
          />
          <View style={{ flex: 1 }}>
            <If
              when={!props.readOnly}
              else={
                <View class="border border-border bg-muted/50" style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 8, justifyContent: "center" }}>
                  <Text style={{ fontSize: 13 }} class="text-secondary-foreground">{strVal() || (f().placeholder ?? "默认字体")}</Text>
                </View>
              }
            >
              <AppMenu
                entries={fontEntries()}
                onSelect={(id) => props.onCommit(id === "__default__" ? "" : id)}
                minWidth={220}
                trigger={
                  <View
                    class="border border-border bg-card hover:bg-muted active:bg-accent"
                    style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 8, flexDirection: "row", alignItems: "center", gap: 6, ...colorTransition(120) }}
                  >
                    <Text
                      style={{ flex: 1, fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }}
                      class={strVal() ? "text-foreground" : "text-muted-foreground"}
                    >
                      {strVal() || (f().placeholder ?? "默认字体")}
                    </Text>
                    <Chevron size={13} tone="muted-foreground" />
                  </View>
                }
              />
            </If>
          </View>
        </View>
      </View>
    );
  }

  if (ty() === "lines") {
    return (
      <LinesField
        ownerKey={props.ownerKey}
        label={f().label}
        description={f().description}
        value={Array.isArray(props.value) ? props.value : []}
        overridden={props.overridden}
        readOnly={props.readOnly}
        restoreTooltip={props.restoreTooltip}
        onCommit={(lines) => props.onCommit(lines)}
        onRestore={props.onRestore}
      />
    );
  }

  if (ty() === "image") {
    return (
      <ImageOptionField
        label={f().label}
        description={f().description}
        placeholder={f().placeholder}
        value={String(props.value ?? "")}
        contestId={props.contestId}
        overridden={props.overridden}
        readOnly={props.readOnly}
        restoreTooltip={props.restoreTooltip}
        onCommit={(rel) => props.onCommit(rel)}
        onRestore={props.onRestore}
      />
    );
  }

  // `string` | `number` | `length` | `color`
  return (
    <TextOptionField
      ownerKey={props.ownerKey}
      field={f()}
      value={props.value}
      overridden={props.overridden}
      readOnly={props.readOnly}
      restoreTooltip={props.restoreTooltip}
      onCommit={props.onCommit}
      onRestore={props.onRestore}
    />
  );
}

function TextOptionField(props: {
  ownerKey: string;
  field: TemplateField;
  value: TemplateOptionValue;
  overridden: boolean;
  readOnly?: boolean;
  restoreTooltip: string;
  onCommit: (v: TemplateOptionValue) => void;
  onRestore: () => void;
}) {
  const strOf = (v: TemplateOptionValue) => (v == null ? "" : String(v));
  const draft = signal(strOf(props.value));
  let focused = false;
  let lastOwner = props.ownerKey;
  let lastValue = strOf(props.value);

  const commit = (raw: string) => {
    if (props.readOnly) return;
    const trimmed = raw.trim();
    if (props.field.type === "number") {
      const n = Number(trimmed);
      if (!Number.isFinite(n)) {
        draft.set(lastValue);
        return;
      }
      const clamped = Math.min(props.field.max ?? Infinity, Math.max(props.field.min ?? -Infinity, n));
      props.onCommit(clamped);
      draft.set(String(clamped));
      return;
    }
    props.onCommit(trimmed);
  };

  effect(() => {
    const owner = props.ownerKey;
    const v = strOf(props.value);
    untracked(() => {
      if (owner !== lastOwner) {
        focused = false;
        lastOwner = owner;
        lastValue = v;
        draft.set(v);
        return;
      }
      lastValue = v;
      if (!focused) draft.set(v);
    });
  });

  const isColor = () => props.field.type === "color";
  const swatchColor = () => {
    const v = draft().trim();
    return /^#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{3}|[0-9a-fA-F]{8})$/.test(v) ? v : "#0000ff";
  };

  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
        <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
          {props.field.label}
        </Text>
        <If when={props.overridden && !props.readOnly}>
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
        </If>
      </View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <View
          style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
          class="bg-primary"
        />
        <If when={isColor()}>
          <View
            class="border border-border"
            style={{ width: 22, height: 22, borderRadius: 5, backgroundColor: swatchColor(), flexShrink: 0 }}
          />
        </If>
        <Input
          readonly={props.readOnly}
          value={draft()}
          placeholder={props.field.placeholder}
          keyboardType={props.field.type === "number" ? "number-pad" : undefined}
          autoCapitalize="none"
          autoCorrect={false}
          submitBehavior="blurAndSubmit"
          onChange={(e) => draft.set(e.value)}
          onFocus={() => {
            if (!props.readOnly) focused = true;
          }}
          onBlur={() => {
            if (!focused) return;
            focused = false;
            commit(untracked(draft));
          }}
          onSubmit={() => {
            commit(untracked(draft));
          }}
          class={
            props.readOnly
              ? "flex-1 h-8 px-2.5 rounded-md border border-border bg-muted/50 text-[13px] text-secondary-foreground placeholder:text-muted-foreground"
              : "flex-1 h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
          }
          style={{
            fontVariant: ["tabular-nums"],
            ...(isColor() || props.field.type === "length" ? { fontFamily: MONO } : undefined),
          }}
        />
      </View>
      <If when={!props.readOnly && (props.field.presets?.length ?? 0) > 0}>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4, paddingTop: 2 }}>
          <Each of={props.field.presets ?? []} key={(p) => p}>
            {(preset) => {
              const active = () => draft().trim() === preset();
              return (
                <View
                  onPress={() => {
                    draft.set(preset());
                    props.onCommit(preset());
                  }}
                  class={active() ? "bg-accent border border-border" : "bg-muted/60 hover:bg-muted border border-border"}
                  style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center", ...colorTransition(100) }}
                >
                  <Text
                    style={{ fontSize: 11, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                    class={active() ? "text-foreground" : "text-secondary-foreground"}
                  >
                    {preset()}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>
      </If>
    </View>
  );
}

function LinesField(props: {
  ownerKey: string;
  label: string;
  description?: string;
  value: string[];
  overridden: boolean;
  readOnly?: boolean;
  restoreTooltip: string;
  onCommit: (lines: string[]) => void;
  onRestore: () => void;
}) {
  const joined = (arr: string[]) => arr.join("\n");
  const draft = signal(joined(props.value));
  const height = signal(84);
  let focused = false;
  let lastOwner = props.ownerKey;

  const commit = () => {
    if (props.readOnly) return;
    const lines = untracked(draft)
      .replace(/\r/g, "")
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    props.onCommit(lines);
  };

  effect(() => {
    const owner = props.ownerKey;
    const next = joined(props.value);
    untracked(() => {
      if (owner !== lastOwner) {
        if (focused) commit();
        focused = false;
        lastOwner = owner;
        draft.set(next);
        return;
      }
      if (!focused) draft.set(next);
    });
  });

  cleanup(() => {
    if (focused) commit();
  });

  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
        <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
          {props.label}
        </Text>
        <If when={props.overridden && !props.readOnly}>
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
        </If>
      </View>
      <View style={{ flexDirection: "row" }}>
        <View
          style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
          class="bg-primary"
        />
        <Input
          multiline
          readonly={props.readOnly}
          value={draft()}
          autoCapitalize="none"
          autoCorrect={false}
          onContentSizeChange={(e) => {
            const next = Math.max(84, Math.min(200, Math.ceil(e.size.height) + 18));
            if (next !== height()) height.set(next);
          }}
          onChange={(e) => draft.set(e.value)}
          onFocus={() => {
            if (!props.readOnly) focused = true;
          }}
          onBlur={() => {
            if (!focused) return;
            focused = false;
            commit();
          }}
          class={
            props.readOnly
              ? "flex-1 px-2.5 py-1.5 rounded-md border border-border bg-muted/50 text-[12.5px] text-secondary-foreground"
              : "flex-1 px-2.5 py-1.5 rounded-md border border-border bg-card text-[12.5px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
          }
          style={{ lineHeight: 18, height: height() }}
        />
      </View>
      <If when={!!props.description}>
        <Text style={{ fontSize: 11.5, lineHeight: 15 }} class="text-muted-foreground">
          {props.description}
        </Text>
      </If>
    </View>
  );
}

function ImageOptionField(props: {
  label: string;
  description?: string;
  placeholder?: string;
  value: string;
  contestId?: string;
  overridden: boolean;
  readOnly?: boolean;
  restoreTooltip: string;
  onCommit: (relPath: string) => void;
  onRestore: () => void;
}) {
  const err = signal<UiError | undefined>(undefined);
  const currentPath = () => props.value.trim();
  const displayName = () => (currentPath() ? pathBase(currentPath()) : "");

  const pickImage = async () => {
    if (props.readOnly) return;
    err.set(undefined);
    try {
      const rel = await preview.pickAndImportTemplateImage(props.contestId ?? null);
      if (rel) props.onCommit(rel);
    } catch (e) {
      const raw = String(e ?? "无法导入图片").replace(/^Error:\s*/i, "").trim();
      err.set({ context: "导入图片", error: raw || "无法导入图片，请检查文件权限后重试。" });
    }
  };

  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", height: 18 }}>
        <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
          {props.label}
        </Text>
        <If when={props.overridden && !props.readOnly}>
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(props.restoreTooltip)} onPress={props.onRestore} />
        </If>
      </View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <View
          style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }}
          class="bg-primary"
        />
        <View
          tooltip={currentPath() ? tip(currentPath()) : undefined}
          class={props.readOnly ? "border border-border bg-muted/50" : "border border-border bg-card"}
          style={{
            flex: 1,
            minWidth: 0,
            height: 32,
            borderRadius: 6,
            paddingLeft: 8,
            paddingRight: 8,
            flexDirection: "row",
            alignItems: "center",
            gap: 6,
            overflow: "hidden",
          }}
        >
          <Icon name="image" size={13} tone={displayName() ? "foreground" : "muted-foreground"} />
          <Text
            style={{
              flex: 1,
              minWidth: 0,
              fontSize: 12.5,
              numberOfLines: 1,
              textOverflow: "ellipsis",
              ...(displayName() ? { fontFamily: MONO } : undefined),
            }}
            class={displayName() ? "text-foreground" : "text-muted-foreground"}
          >
            {displayName() || (props.placeholder ?? "未选择图片")}
          </Text>
        </View>
        <If when={!props.readOnly}>
          <Btn label="选择图片" onPress={() => void pickImage()} />
          <If when={!!currentPath()}>
            <IconBtn icon="x" size={28} iconSize={13} tooltip={tip("清除图片")} onPress={() => props.onCommit("")} />
          </If>
        </If>
      </View>
      <If when={!!props.description}>
        <Text style={{ fontSize: 11.5, lineHeight: 15 }} class="text-muted-foreground">
          {props.description}
        </Text>
      </If>
      <If when={err()}>
        <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
      </If>
    </View>
  );
}

/** Lightweight modal to fork an existing workspace template into a new editable template. */
export function ForkTemplateModal(props: {
  open: boolean;
  sourceId: string;
  sourceName: string;
  existingIds?: string[];
  onCreated: (newId: string) => void;
  onClose: () => void;
}) {
  const slug = signal("");
  const submitted = signal(false);
  const err = signal<UiError | undefined>(undefined);

  const suggestForkSlug = (base: string): string => {
    const existing = new Set(props.existingIds ?? []);
    const first = `${base}-custom`;
    if (!existing.has(first)) return first;
    for (let i = 2; i < 100; i++) {
      const candidate = `${base}-custom-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return first;
  };

  effect(() => {
    if (props.open) {
      untracked(() => {
        slug.set(suggestForkSlug(props.sourceId));
        submitted.set(false);
        err.set(undefined);
      });
    }
  });

  const validationError = computed(() => {
    const id = slug().trim();
    if (!id) return submitted() ? "请输入新模板标识（如 my-icpc）" : undefined;
    if (!validId(id)) return "标识只能包含小写字母、数字、- 和 _，并以字母或数字开头";
    if ((props.existingIds ?? []).includes(id)) return "该模板标识已存在，请更换名称";
    return undefined;
  });

  const canSubmit = () => {
    const id = slug().trim();
    return !!id && validId(id) && !(props.existingIds ?? []).includes(id);
  };

  const submit = () => {
    submitted.set(true);
    if (!canSubmit()) return;
    const id = slug().trim();
    const res = preview.forkTemplate(props.sourceId, id);
    if (!res.ok) {
      err.set(errorOf(res, "新建模板副本"));
      return;
    }
    props.onCreated(id);
    props.onClose();
  };

  return (
    <Modal open={props.open} onClose={props.onClose} width={380}>
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View class="bg-muted border border-border" style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}>
            <Icon name="copy" size={15} tone="foreground" />
          </View>
          <View style={{ flex: 1 }}>
            <Text style={{ fontSize: 14.5, fontWeight: 600 }} class="text-foreground">
              新建自定义模板
            </Text>
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {`基于「${props.sourceName}」创建一份工作区共享副本`}
            </Text>
          </View>
        </View>
        <View style={{ gap: 5 }}>
          <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
            模板标识
          </Text>
          <Input
            value={slug()}
            placeholder="my-template"
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="blurAndSubmit"
            onChange={(e) => {
              slug.set(e.value);
              if (err()) err.set(undefined);
            }}
            onSubmit={submit}
            class={
              validationError()
                ? "h-8 px-2.5 rounded-md border border-destructive bg-card text-[13px] focus:border-destructive transition-colors"
                : "h-8 px-2.5 rounded-md border border-border bg-card text-[13px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
            }
            style={{ fontFamily: MONO }}
          />
          <Text
            style={{ fontSize: 11.5, lineHeight: 16 }}
            class={validationError() ? "text-destructive" : "text-muted-foreground"}
          >
            {validationError() ?? "小写字母、数字、- 或 _，最多 64 字符"}
          </Text>
        </View>
        <If when={err()}>
          <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
        </If>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Btn label="取消" onPress={props.onClose} />
          <Btn label="创建" variant="primary" disabled={!canSubmit()} onPress={submit} />
        </View>
      </View>
    </Modal>
  );
}

/** Modal for installing a bundled preset seed (`olymp-classic`, `cnoi-classic`, `minimal`) into the workspace. */
export function InstallPresetModal(props: {
  open: boolean;
  presets: PresetInfo[];
  existingIds: string[];
  initialTargetName?: string;
  onInstalled: (installedId: string) => void;
  onClose: () => void;
}) {
  const selectedPreset = signal("");
  const targetName = signal("");
  const submitted = signal(false);
  const userEditedName = signal(false);
  const err = signal<UiError | undefined>(undefined);

  const suggestTarget = (presetId: string): string => {
    const existing = new Set(props.existingIds);
    if (!existing.has(presetId)) return presetId;
    for (let i = 2; i < 100; i++) {
      const candidate = `${presetId}-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return `${presetId}-copy`;
  };

  effect(() => {
    if (props.open) {
      untracked(() => {
        const existing = new Set(props.existingIds);
        const firstMissing = props.presets.find((p) => !existing.has(p.id));
        const initial = (firstMissing ?? props.presets[0])?.id ?? "olymp-classic";
        selectedPreset.set(initial);
        const preferred = props.initialTargetName?.trim();
        if (preferred && validId(preferred) && !existing.has(preferred)) {
          targetName.set(preferred);
          userEditedName.set(true);
        } else {
          targetName.set(suggestTarget(initial));
          userEditedName.set(false);
        }
        submitted.set(false);
        err.set(undefined);
      });
    }
  });

  const choosePreset = (id: string) => {
    selectedPreset.set(id);
    if (!userEditedName()) {
      targetName.set(suggestTarget(id));
    }
    if (err()) err.set(undefined);
  };

  const validationError = computed(() => {
    const name = targetName().trim();
    if (!name) return submitted() ? "请输入模板标识（如 olymp-classic）" : undefined;
    if (!validId(name)) return "标识只能包含小写字母、数字、- 和 _，并以字母或数字开头";
    if (props.existingIds.includes(name)) return "该模板标识已存在，请更换名称";
    return undefined;
  });

  const canSubmit = () => {
    const name = targetName().trim();
    return !!selectedPreset() && !!name && validId(name) && !props.existingIds.includes(name);
  };

  const submit = () => {
    submitted.set(true);
    if (!canSubmit()) return;
    const preset = selectedPreset();
    const name = targetName().trim();
    const res = preview.installPresetTemplate(preset, name);
    if (!res.ok) {
      err.set(errorOf(res, "从预设安装模板"));
      return;
    }
    props.onInstalled(name);
    props.onClose();
  };

  return (
    <Modal open={props.open} onClose={props.onClose} width={420}>
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View class="bg-muted border border-border" style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}>
            <Icon name="plus" size={15} tone="foreground" />
          </View>
          <View style={{ flex: 1 }}>
            <Text style={{ fontSize: 14.5, fontWeight: 600 }} class="text-foreground">
              从预设添加模板
            </Text>
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              将预置模板复制到当前工作区，安装后可自由修改
            </Text>
          </View>
        </View>

        <View style={{ gap: 6 }}>
          <Each of={props.presets} key={(p) => p.id}>
            {(preset) => {
              const active = () => selectedPreset() === preset().id;
              return (
                <View
                  onPress={() => choosePreset(preset().id)}
                  class={active() ? "border border-primary bg-accent/60" : "border border-border bg-card hover:bg-muted/60"}
                  style={{
                    borderRadius: 8,
                    paddingLeft: 12,
                    paddingRight: 12,
                    paddingTop: 9,
                    paddingBottom: 9,
                    gap: 2,
                    ...colorTransition(120),
                  }}
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Text style={{ flex: 1, fontSize: 13, fontWeight: 500 }} class="text-foreground">
                      {preset().name}
                    </Text>
                    <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-muted-foreground">
                      {preset().id}
                    </Text>
                  </View>
                  <Text style={{ fontSize: 11.5, lineHeight: 16 }} class="text-secondary-foreground">
                    {preset().description}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>

        <View style={{ gap: 5 }}>
          <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
            保存为模板标识
          </Text>
          <Input
            value={targetName()}
            placeholder="olymp-classic"
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="blurAndSubmit"
            onChange={(e) => {
              targetName.set(e.value);
              userEditedName.set(true);
              if (err()) err.set(undefined);
            }}
            onSubmit={submit}
            class={
              validationError()
                ? "h-8 px-2.5 rounded-md border border-destructive bg-card text-[13px] focus:border-destructive transition-colors"
                : "h-8 px-2.5 rounded-md border border-border bg-card text-[13px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
            }
            style={{ fontFamily: MONO }}
          />
          <Text
            style={{ fontSize: 11.5, lineHeight: 16 }}
            class={validationError() ? "text-destructive" : "text-muted-foreground"}
          >
            {validationError() ?? "小写字母、数字、- 或 _，安装后供整个工作区选用"}
          </Text>
        </View>

        <If when={err()}>
          <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
        </If>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Btn label="取消" onPress={props.onClose} />
          <Btn label="添加" variant="primary" disabled={!canSubmit()} onPress={submit} />
        </View>
      </View>
    </Modal>
  );
}
