// The split export button (§6.5): its main body always triggers PDF export for the
// current book (never morphs into "reveal"), shows a restrained in-place `✓ 已导出`
// confirmation for 1.8 s after a successful export with zero width jitter, and delays
// the spinner by 150 ms. The chevron opens an `AppMenu` for either book and "在文件夹中显示".

import { cleanup, effect, signal, Text, View } from "nsh";
import type { ExportBookStatusJs } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { DelayedSpinner, FOCUS_RING } from "../kit";
import { revealState, type Book } from "../logic";
import { AppMenu, type MenuEntry } from "../Menu";
import type { OjPackageFormat } from "../store";
import type { RegisteredExporter } from "../tools/types";

import { colorTransition, CONTROL, FONT_SIZE, opacityTransition, RADIUS, SPACE } from "../tokens";
const CONFIRM_MS = 1800;

export function ExportButton(props: {
  status: ExportBookStatusJs | undefined;
  book: Book;
  busy: Book | OjPackageFormat | undefined;
  /** Incremented by the caller each time an export finishes cleanly. */
  exportedSeq?: number;
  /** True when an OJ package was exported in this session and can be revealed in folder. */
  canRevealOj?: boolean;
  /** Demotes the button to outline/secondary styling when another primary CTA owns the screen. */
  secondary?: boolean;
  /** Enabled plugin exporters (omitted from menu when empty). */
  pluginExporters?: readonly RegisteredExporter[];
  onExport: (book: Book) => void;
  onExportOj?: (format: OjPackageFormat) => void;
  onExportPlugin?: (pluginId: string, exporterId: string) => void;
  onReveal: (book: Book) => void;
}) {
  const reveal = () => revealState(props.status, props.book);
  const busy = () => props.busy !== undefined;

  // Delay spinner layout space by 150 ms so fast exports (<150 ms) never shift the label.
  const showSpinner = signal(false);
  let spinTimer: number | undefined;
  effect(() => {
    const active = busy();
    clearTimeout(spinTimer);
    if (!active) showSpinner.set(false);
    else spinTimer = setTimeout(() => showSpinner.set(true), 150);
  });

  // Brief in-place confirmation after an explicit user export (§6.5).
  const confirmed = signal(false);
  let lastSeq = props.exportedSeq ?? 0;
  let confirmTimer: number | undefined;
  effect(() => {
    const seq = props.exportedSeq ?? 0;
    if (seq > lastSeq) {
      lastSeq = seq;
      confirmed.set(true);
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => confirmed.set(false), CONFIRM_MS);
    }
  });
  effect(() => {
    if (busy()) {
      clearTimeout(confirmTimer);
      confirmed.set(false);
    }
  });

  cleanup(() => {
    clearTimeout(spinTimer);
    clearTimeout(confirmTimer);
  });

  const entries = (): MenuEntry[] => {
    const rev = reveal();
    const list: MenuEntry[] = [
      {
        id: "statements",
        label: t("export.statements"),
        icon: "fileDown",
        keys: props.book === "statements" ? "mod+e" : undefined,
      },
      {
        id: "editorial",
        label: t("export.editorial"),
        icon: "fileDown",
        keys: props.book === "editorial" ? "mod+e" : undefined,
      },
      {
        id: "reveal",
        label: t("common.revealInFolder"),
        icon: "folderOpen",
        disabled: rev === "none" && !props.canRevealOj,
        note: rev === "stale" && !props.canRevealOj ? t("export.stale") : undefined,
      },
    ];
    if (props.onExportOj) {
      list.push(
        { separator: true, id: "sep-oj" },
        {
          id: "domjudge",
          label: t("export.domjudge"),
          icon: "database",
        },
        {
          id: "hydro",
          label: t("export.hydro"),
          icon: "database",
        },
      );
    }
    if (props.onExportPlugin && props.pluginExporters && props.pluginExporters.length > 0) {
      list.push({ separator: true, id: "sep-plugin-exporters" });
      for (const exp of props.pluginExporters) {
        list.push({
          id: `plugin-exp:${exp.plugin_id}:${exp.exporter.id}`,
          label: exp.exporter.name,
          note: exp.plugin_name,
          icon: "sparkles",
        });
      }
    }
    return list;
  };

  const onMenuSelect = (id: string) => {
    if (busy()) return;
    if (id === "statements") props.onExport("statements");
    else if (id === "editorial") props.onExport("editorial");
    else if (id === "reveal") props.onReveal(props.book);
    else if (id === "domjudge") props.onExportOj?.("domjudge");
    else if (id === "hydro") props.onExportOj?.("hydro");
    else if (id.startsWith("plugin-exp:")) {
      const rest = id.slice("plugin-exp:".length);
      const sep = rest.indexOf(":");
      if (sep > 0) {
        props.onExportPlugin?.(rest.slice(0, sep), rest.slice(sep + 1));
      }
    }
  };

  const sec = () => Boolean(props.secondary);
  const fgTone = () => (sec() ? "foreground" : "primary-foreground");
  const fgClass = () => (sec() ? "text-foreground" : "text-primary-foreground");
  const btnClass = (disabled: boolean) =>
    sec()
      ? disabled
        ? "bg-card"
        : `bg-card hover:bg-muted active:bg-accent ${FOCUS_RING}`
      : disabled
        ? "bg-primary"
        : `bg-primary hover:bg-sidebar-primary active:opacity-90 ${FOCUS_RING}`;

  const chevronTrigger = (disabled: boolean) => (
    <View
      testID="export-menu"
      disabled={disabled}
      class={btnClass(disabled)}
      style={{
        width: 24,
        height: CONTROL.md,
        alignItems: "center",
        justifyContent: "center",
        ...colorTransition(),
      }}
    >
      <Icon name="chevronDown" size="sm" tone={fgTone()} />
    </View>
  );

  return (
    <View
      style={{
        flexDirection: "row",
        height: CONTROL.md,
        borderRadius: RADIUS.sm,
        borderWidth: sec() ? 1 : 0,
        overflow: "hidden",
        opacity: busy() ? 0.8 : 1,
        ...opacityTransition(),
      }}
      class={sec() ? "bg-card border-border" : "bg-primary"}
    >
      <View
        testID="export-main"
        disabled={busy()}
        onPress={() => !busy() && props.onExport(props.book)}
        class={btnClass(busy())}
        style={{
          minWidth: 96,
          paddingLeft: SPACE[12],
          paddingRight: SPACE[12],
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          ...colorTransition(),
        }}
      >
        <View style={{ width: showSpinner() ? 12 : 0, marginRight: showSpinner() ? SPACE[6] : SPACE[0], overflow: "hidden", alignItems: "center", justifyContent: "center" }}>
          <DelayedSpinner active={busy()} class={sec() ? "border-foreground" : "border-primary-foreground"} />
        </View>
        <If when={confirmed() && !busy()}>
          <Icon name="check" size="sm" tone={fgTone()} style={{ marginRight: SPACE[4] }} />
        </If>
        <Text style={{ fontSize: FONT_SIZE.body }} class={fgClass()}>
          {confirmed() && !busy() ? t("export.exported") : t("export.pdf")}
        </Text>
      </View>
      <View
        style={{ width: 1, alignSelf: "stretch", backgroundColor: sec() ? undefined : "rgba(255,255,255,0.22)" }}
        class={sec() ? "bg-border" : undefined}
      />
      <If when={!busy()} else={chevronTrigger(true)}>
        <AppMenu entries={entries()} onSelect={onMenuSelect} align="end" minWidth={210} trigger={chevronTrigger(false)} />
      </If>
    </View>
  );
}
