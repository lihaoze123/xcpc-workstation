// The split export button (§6.5): its main body always triggers PDF export for the
// current book (never morphs into "reveal"), shows a restrained in-place `✓ 已导出`
// confirmation for 1.8 s after a successful export with zero width jitter, and delays
// the spinner by 150 ms. The chevron opens an `AppMenu` for either book and "在文件夹中显示".

import { cleanup, effect, signal, Text, View } from "nsh";
import type { ExportBookStatusJs } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { DelayedSpinner } from "../kit";
import { revealState, type Book } from "../logic";
import { AppMenu, type MenuEntry } from "../Menu";
import { EASE_STANDARD, ms } from "../theme";

const CONFIRM_MS = 1800;

export function ExportButton(props: {
  status: ExportBookStatusJs | undefined;
  book: Book;
  busy: Book | undefined;
  /** Incremented by the caller each time an export finishes cleanly. */
  exportedSeq?: number;
  onExport: (book: Book) => void;
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

  const entries = (): MenuEntry[] => [
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
    { separator: true, id: "sep" },
    {
      id: "reveal",
      label: t("common.revealInFolder"),
      icon: "folderOpen",
      disabled: reveal() === "none",
      note: reveal() === "stale" ? t("export.stale") : undefined,
    },
  ];

  const onMenuSelect = (id: string) => {
    if (busy()) return;
    if (id === "statements") props.onExport("statements");
    else if (id === "editorial") props.onExport("editorial");
    else if (id === "reveal") props.onReveal(props.book);
  };

  const chevronTrigger = (disabled: boolean) => (
    <View
      testID="export-menu"
      disabled={disabled}
      class={disabled ? "bg-primary" : "bg-primary hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"}
      style={{
        width: 24,
        height: 28,
        alignItems: "center",
        justifyContent: "center",
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
    >
      <Icon name="chevronDown" size={14} tone="primary-foreground" />
    </View>
  );

  return (
    <View
      style={{
        flexDirection: "row",
        height: 28,
        borderRadius: 6,
        overflow: "hidden",
        opacity: busy() ? 0.8 : 1,
        transitionProperty: "opacity",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
      class="bg-primary"
    >
      <View
        testID="export-main"
        disabled={busy()}
        onPress={() => !busy() && props.onExport(props.book)}
        class={busy() ? "bg-primary" : "bg-primary hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"}
        style={{
          minWidth: 96,
          paddingLeft: 12,
          paddingRight: 12,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          transitionProperty: "colors",
          transitionDuration: ms(120),
          transitionTimingFunction: EASE_STANDARD,
        }}
      >
        <View style={{ width: showSpinner() ? 12 : 0, marginRight: showSpinner() ? 6 : 0, overflow: "hidden", alignItems: "center", justifyContent: "center" }}>
          <DelayedSpinner active={busy()} class="border-primary-foreground" />
        </View>
        <If when={confirmed() && !busy()}>
          <Icon name="check" size={14} tone="primary-foreground" style={{ marginRight: 5 }} />
        </If>
        <Text style={{ fontSize: 13 }} class="text-primary-foreground">
          {confirmed() && !busy() ? t("export.exported") : t("export.pdf")}
        </Text>
      </View>
      <View style={{ width: 1, alignSelf: "stretch", backgroundColor: "rgba(255,255,255,0.22)" }} />
      <If when={!busy()} else={chevronTrigger(true)}>
        <AppMenu entries={entries()} onSelect={onMenuSelect} align="end" minWidth={210} trigger={chevronTrigger(false)} />
      </If>
    </View>
  );
}
