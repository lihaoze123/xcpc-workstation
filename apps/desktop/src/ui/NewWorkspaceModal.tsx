// Modal for creating a new workspace: lets the user choose between initializing from
// the built-in example template (2 curated problems + 1 contest) or creating an empty
// workspace before opening the folder picker.

import { colorScheme, Text, View } from "nsh";
import { Icon, type IconName } from "./Icon";
import { IconBtn, tip } from "./kit";
import { Modal } from "./Modal";
import { colorTransition } from "./motion";
import { newWorkspace, openExampleWorkspace } from "./store";

interface ChoiceOption {
  readonly id: "template" | "empty";
  readonly testID: string;
  readonly icon: IconName;
  readonly title: string;
  readonly description: string;
  readonly run: () => void;
}

export function NewWorkspaceModal(props: { open: boolean; onClose: () => void }) {
  const dark = () => colorScheme() === "dark";

  const options: readonly ChoiceOption[] = [
    {
      id: "template",
      testID: "new-workspace-template",
      icon: "fileText",
      title: "从示例模板创建",
      description: "内置 2 道示例题与 1 场示例比赛，开箱体验排版",
      run: () => {
        props.onClose();
        void openExampleWorkspace();
      },
    },
    {
      id: "empty",
      testID: "new-workspace-empty",
      icon: "folderPlus",
      title: "创建空工作区",
      description: "干净的空白目录，适合直接开始自己的题库与比赛",
      run: () => {
        props.onClose();
        void newWorkspace();
      },
    },
  ];

  return (
    <Modal open={props.open} onClose={props.onClose} width={440}>
      <View testID="new-workspace-modal" style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
          <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
            新建工作区
          </Text>
          <IconBtn icon="x" size={26} iconSize={14} tooltip={tip("关闭", "escape")} onPress={props.onClose} />
        </View>

        <View style={{ gap: 8 }}>
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
                  class="bg-background border border-border hover:bg-muted/70 active:bg-accent"
                  style={{
                    borderRadius: 10,
                    paddingTop: 12,
                    paddingBottom: 12,
                    paddingLeft: 12,
                    paddingRight: 12,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 12,
                    ...colorTransition(120),
                  }}
                >
                  <View
                    style={{
                      width: 34,
                      height: 34,
                      borderRadius: 8,
                      backgroundColor: iconBg(),
                      alignItems: "center",
                      justifyContent: "center",
                      flexShrink: 0,
                    }}
                  >
                    <Icon name={opt().icon} size={17} style={{ tintColor: iconFg() }} />
                  </View>
                  <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                    <Text style={{ fontSize: 13.5, fontWeight: 500, lineHeight: 18 }} class="text-foreground">
                      {opt().title}
                    </Text>
                    <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-muted-foreground">
                      {opt().description}
                    </Text>
                  </View>
                  <Icon name="chevronRight" size={15} tone="muted-foreground" />
                </View>
              );
            }}
          </Each>
        </View>
      </View>
    </Modal>
  );
}
