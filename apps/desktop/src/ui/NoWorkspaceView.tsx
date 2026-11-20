// What shows while no workspace is open (Phase 2.5 seam #1: the brand page replaces
// this component alone). Two ways in and the recent workspaces; when the remembered
// workspace is gone, a fixed slot under the entries says so (a fresh launch says
// nothing).

import { Text, View } from "nsh";
import { Btn, tip } from "./kit";
import { Icon } from "./Icon";
import { ErrorDisplay } from "./ErrorDisplay";
import { abbreviateHome, pathBase } from "./logic";
import { EASE_STANDARD, MONO, ms } from "./theme";
import { info, newWorkspace, openError, openRecent, pickWorkspace, preview } from "./store";

export function NoWorkspaceView() {
  const recent = () => {
    void info();
    return preview.recentWorkspaces().slice(0, 5);
  };
  return (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }} class="bg-background">
      <View style={{ width: 300, maxWidth: "90%", gap: 8 }}>
        <View style={{ flexDirection: "row", justifyContent: "center", gap: 6, marginBottom: 20 }}>
          <Text style={{ fontSize: 22, fontWeight: 700 }}>XCPC</Text>
          <Text style={{ fontSize: 22 }} class="text-secondary-foreground">
            Workstation
          </Text>
        </View>
        <Btn testID="new-workspace" variant="primary" size="md" icon="folderPlus" label="新建工作区" onPress={() => void newWorkspace()} />
        <Btn testID="open-workspace" variant="outline" size="md" icon="folderOpen" label="打开已有文件夹" onPress={() => void pickWorkspace()} tooltip={tip("打开已有文件夹", "mod+o")} />
        <View style={{ minHeight: 36, justifyContent: "center", paddingTop: 4, paddingBottom: 4 }}>
          <If
            when={openError()}
            else={
              info().lastWorkspaceMissing ? (
                <Text style={{ fontSize: 12, textAlign: "center" }} class="text-secondary-foreground">
                  上次的工作区已不存在
                </Text>
              ) : null
            }
          >
            <ErrorDisplay error={openError()} align="center" onDismiss={() => openError.set(undefined)} />
          </If>
        </View>
        <If when={recent().length > 0}>
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 12, paddingLeft: 8, paddingBottom: 4 }} class="text-muted-foreground">
              最近打开
            </Text>
            <Each of={recent()} key={(p) => p}>
              {(path) => (
                <View
                  onPress={() => openRecent(path())}
                  tooltip={tip(path())}
                  class="hover:bg-muted active:bg-accent"
                  style={{
                    height: 44,
                    paddingLeft: 8,
                    paddingRight: 8,
                    borderRadius: 6,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 8,
                    transitionProperty: "colors",
                    transitionDuration: ms(120),
                    transitionTimingFunction: EASE_STANDARD,
                  }}
                >
                  <Icon name="folder" size={15} tone="muted-foreground" />
                  <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 13, lineHeight: 17, fontWeight: 500 }}>
                      {pathBase(path())}
                    </Text>
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 11.5, lineHeight: 15, fontFamily: MONO }} class="text-muted-foreground">
                      {abbreviateHome(path())}
                    </Text>
                  </View>
                </View>
              )}
            </Each>
          </View>
        </If>
      </View>
    </View>
  );
}
