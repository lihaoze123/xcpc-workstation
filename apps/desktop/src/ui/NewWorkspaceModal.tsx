// Modal for creating a new workspace: lets the user choose a default publishing
// template (XCPC `olymp-classic` or OI `cnoi-classic`) and then either initialize from
// the built-in example workspace or create an empty workspace.

import { colorScheme, signal, Text, View } from "nsh";
import { t, type MessageKey } from "./i18n";
import { Icon, type IconName } from "./Icon";
import { FOCUS_RING, IconBtn, tip } from "./kit";
import { Modal } from "./Modal";
import { colorTransition } from "./motion";
import { newWorkspace, openExampleWorkspace } from "./store";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
type DefaultTemplate = "olymp-classic" | "cnoi-classic";

interface TemplateChoice {
  readonly id: DefaultTemplate;
  readonly labelKey: MessageKey;
  readonly sub: string;
}

const TEMPLATES: readonly TemplateChoice[] = [
  { id: "olymp-classic", labelKey: "newWorkspace.tpl.olympClassic", sub: "olymp-classic" },
  { id: "cnoi-classic", labelKey: "newWorkspace.tpl.cnoiClassic", sub: "cnoi-classic" },
];

interface ChoiceOption {
  readonly id: "template" | "empty";
  readonly testID: string;
  readonly icon: IconName;
  readonly titleKey: MessageKey;
  readonly descriptionKey: MessageKey;
  readonly run: () => void;
}

export function NewWorkspaceModal(props: { open: boolean; onClose: () => void }) {
  const dark = () => colorScheme() === "dark";
  const selectedTemplate = signal<DefaultTemplate>("olymp-classic");

  const options: readonly ChoiceOption[] = [
    {
      id: "template",
      testID: "new-workspace-template",
      icon: "fileText",
      titleKey: "newWorkspace.fromExample.title",
      descriptionKey: "newWorkspace.fromExample.desc",
      run: () => {
        const tpl = selectedTemplate();
        props.onClose();
        void openExampleWorkspace(tpl);
      },
    },
    {
      id: "empty",
      testID: "new-workspace-empty",
      icon: "folderPlus",
      titleKey: "newWorkspace.empty.title",
      descriptionKey: "newWorkspace.empty.desc",
      run: () => {
        const tpl = selectedTemplate();
        props.onClose();
        void newWorkspace(tpl);
      },
    },
  ];

  return (
    <Modal open={props.open} onClose={props.onClose} width={440}>
      <View testID="new-workspace-modal" style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
          <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
            {t("newWorkspace.title")}
          </Text>
          <IconBtn icon="x" size="sm" tooltip={tip(t("common.close"), "escape")} onPress={props.onClose} />
        </View>

        <View style={{ gap: SPACE[6] }}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
              {t("newWorkspace.defaultTemplate")}
            </Text>
            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
              {t("newWorkspace.defaultTemplateHint")}
            </Text>
          </View>
          <View
            style={{ flexDirection: "row", gap: SPACE[6], padding: SPACE[4], borderRadius: RADIUS.md }}
            class="bg-muted border border-border"
          >
            <Each of={TEMPLATES} key={(t) => t.id}>
              {(tpl) => {
                const active = () => selectedTemplate() === tpl().id;
                return (
                  <View
                    testID={`new-workspace-tpl-${tpl().id}`}
                    onPress={() => selectedTemplate.set(tpl().id)}
                    class={active() ? `bg-card border border-border ${FOCUS_RING}` : `hover:bg-background/50 ${FOCUS_RING}`}
                    style={{
                      flex: 1,
                      borderRadius: RADIUS.sm,
                      paddingTop: SPACE[8],
                      paddingBottom: SPACE[8],
                      paddingLeft: SPACE[12],
                      paddingRight: SPACE[12],
                      alignItems: "center",
                      gap: SPACE[2],
                      borderWidth: active() ? 1 : 0,
                      ...colorTransition(120),
                    }}
                  >
                    <Text
                      style={{ fontSize: FONT_SIZE.caption, fontWeight: active() ? FONT_WEIGHT.semibold : FONT_WEIGHT.regular, lineHeight: LINE_HEIGHT.caption }}
                      class={active() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {t(tpl().labelKey)}
                    </Text>
                    <Text style={{ fontSize: FONT_SIZE.micro, lineHeight: LINE_HEIGHT.micro }} class="text-muted-foreground">
                      {tpl().sub}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </View>

        <View style={{ gap: SPACE[8] }}>
          <Each of={options} key={(o) => o.id}>
            {(opt) => {
              const isTemplate = () => opt().id === "template";
              const iconBg = () =>
                isTemplate()
                  ? dark()
                    ? "rgba(68, 178, 178, 0.16)"
                    : "rgba(24, 118, 124, 0.10)"
                  : dark()
                    ? "rgba(255, 255, 255, 0.07)"
                    : "rgba(15, 23, 42, 0.055)";
              const iconFg = () =>
                isTemplate()
                  ? dark()
                    ? "#63c4c4"
                    : "#187076"
                  : dark()
                    ? "#d1d5db"
                    : "#475569";
              return (
                <View
                  testID={opt().testID}
                  onPress={() => opt().run()}
                  class={`bg-background border border-border hover:bg-muted/70 active:bg-accent ${FOCUS_RING}`}
                  style={{
                    borderRadius: RADIUS.lg,
                    paddingTop: SPACE[12],
                    paddingBottom: SPACE[12],
                    paddingLeft: SPACE[12],
                    paddingRight: SPACE[12],
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[12],
                    ...colorTransition(120),
                  }}
                >
                  <View
                    style={{
                      width: 34,
                      height: CONTROL.lg,
                      borderRadius: RADIUS.md,
                      backgroundColor: iconBg(),
                      alignItems: "center",
                      justifyContent: "center",
                      flexShrink: 0,
                    }}
                  >
                    <Icon name={opt().icon} size="md" color={iconFg()} />
                  </View>
                  <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                    <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, lineHeight: LINE_HEIGHT.body }} class="text-foreground">
                      {t(opt().titleKey)}
                    </Text>
                    <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-muted-foreground">
                      {t(opt().descriptionKey)}
                    </Text>
                  </View>
                  <Icon name="chevronRight" size="md" tone="muted-foreground" />
                </View>
              );
            }}
          </Each>
        </View>
      </View>
    </Modal>
  );
}
