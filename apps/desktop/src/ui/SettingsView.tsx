// Cmd+3: grouped cards, each titled with one line on what it is for. The workspace (its
// folder, shown in the file manager; the recent ones; opening or making another) and
// the AI assistant connection (copy the configuration; the JSON itself folded away).

import { cleanup, Clipboard, effect, Scroll, signal, Text, View } from "nsh";
import { Btn, tip } from "./kit";
import { Icon } from "./Icon";
import { ErrorDisplay } from "./ErrorDisplay";
import { Section, SectionRow } from "./layout";
import { abbreviateHome, pathBase } from "./logic";
import { EASE_STANDARD, MONO, ms } from "./theme";
import {
  errorOf,
  info,
  newWorkspace,
  openError,
  openRecent,
  pickWorkspace,
  preview,
  revision,
  workspaceVersion,
  type UiError,
} from "./store";

/** A folder: its name over its path, with the full path on hover. */
function Folder(props: { path: string; note?: string }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }} tooltip={tip(props.path)}>
      <Icon name="folder" size={18} />
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: 13.5, lineHeight: 18, fontWeight: 500 }}>
            {pathBase(props.path)}
          </Text>
          {props.note && (
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {props.note}
            </Text>
          )}
        </View>
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, lineHeight: 16, fontFamily: MONO }} class="text-muted-foreground">
          {abbreviateHome(props.path)}
        </Text>
      </View>
    </View>
  );
}

export function SettingsView() {
  const copied = signal(false);
  const showJson = signal(false);
  const revealError = signal<UiError | undefined>(undefined);
  const snippet = () => {
    void revision();
    return info().root ? preview.mcpConfigSnippet() : "";
  };
  const recent = () => {
    void info();
    return preview.recentWorkspaces().filter((p) => p !== info().root).slice(0, 5);
  };

  let copyTimer: number | undefined;
  cleanup(() => clearTimeout(copyTimer));

  // Clear stale reveal errors when switching workspaces.
  effect(() => {
    void workspaceVersion();
    revealError.set(undefined);
  });

  const copy = () => {
    void Clipboard.writeText(snippet()).then(() => {
      copied.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copied.set(false), 1500);
    });
  };
  const reveal = async () => {
    const res = await preview.revealInFolder(info().root);
    revealError.set(res.ok ? undefined : errorOf(res));
  };

  const sectionError = () => openError() ?? revealError();
  const clearErrors = () => {
    openError.set(undefined);
    revealError.set(undefined);
  };

  return (
    <Scroll style={{ flex: 1 }} class="bg-background">
      <View style={{ width: 640, maxWidth: "100%", alignSelf: "center", paddingTop: 32, paddingBottom: 32, paddingLeft: 24, paddingRight: 24, gap: 28 }}>
        <Section title="工作区" description="题目和比赛都保存在这个文件夹里，可以用 Git 管理。">
          <SectionRow first trailing={<Btn label="在文件夹中显示" icon="folderOpen" onPress={() => void reveal()} />}>
            <Folder path={info().root} note={info().readOnly ? "只读" : undefined} />
          </SectionRow>
          <If when={recent().length > 0}>
            <View style={{ paddingLeft: 14, paddingTop: 10, paddingBottom: 2, borderTopWidth: 1 }} class="border-border">
              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                最近打开
              </Text>
            </View>
            <View style={{ padding: 4, paddingTop: 2 }}>
              <Each of={recent()} key={(p) => p}>
                {(path) => (
                  <View
                    onPress={() => openRecent(path())}
                    class="hover:bg-muted active:bg-accent"
                    style={{
                      height: 48,
                      paddingLeft: 10,
                      paddingRight: 10,
                      borderRadius: 8,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 8,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <View style={{ flex: 1, minWidth: 0 }}>
                      <Folder path={path()} />
                    </View>
                    <Icon name="arrowRight" size={15} tone="muted-foreground" />
                  </View>
                )}
              </Each>
            </View>
          </If>
          <If when={sectionError()}>
            <View style={{ paddingLeft: 14, paddingRight: 14, paddingTop: 10, paddingBottom: 10, borderTopWidth: 1 }} class="border-border bg-muted/40">
              <ErrorDisplay error={sectionError()} onDismiss={clearErrors} />
            </View>
          </If>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: 8, padding: 12, borderTopWidth: 1 }} class="border-border bg-muted/40">
            <Btn label="打开已有文件夹…" icon="folderOpen" onPress={() => void pickWorkspace()} />
            <Btn label="新建工作区…" icon="folderPlus" onPress={() => void newWorkspace()} />
          </View>
        </Section>
        <Section
          title="AI 助手接入"
          description={
            info().readOnly
              ? "当前工作区以只读方式打开。如需限制 AI 助手仅读取，可在配置参数中加入 --read-only。"
              : "让 Claude 等 AI 助手直接读写这个工作区里的题目和比赛。"
          }
        >
          <SectionRow first trailing={<Btn label={copied() ? "已复制" : "复制配置"} icon={copied() ? "check" : "copy"} variant="primary" onPress={copy} style={{ minWidth: 104 }} />}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
              <Icon name="bot" size={18} />
              <Text style={{ fontSize: 13, lineHeight: 18 }}>把这段配置粘贴到助手的 MCP 设置里</Text>
            </View>
          </SectionRow>
          <View
            onPress={() => showJson.set(!showJson())}
            class="hover:bg-muted active:bg-accent border-border"
            style={{
              height: 36,
              paddingLeft: 14,
              flexDirection: "row",
              alignItems: "center",
              gap: 6,
              borderTopWidth: 1,
              transitionProperty: "colors",
              transitionDuration: ms(120),
              transitionTimingFunction: EASE_STANDARD,
            }}
          >
            <Icon
              name="chevronDown"
              size={14}
              tone="muted-foreground"
              style={{
                transform: showJson() ? "rotate(0deg)" : "rotate(-90deg)",
                transitionProperty: "transform",
                transitionDuration: ms(120),
                transitionTimingFunction: EASE_STANDARD,
              }}
            />
            <Text style={{ fontSize: 12.5 }} class="text-secondary-foreground">
              {showJson() ? "收起配置内容" : "查看配置内容"}
            </Text>
          </View>
          <If when={showJson()}>
            <View style={{ borderTopWidth: 1 }} class="border-border bg-muted/40">
              <Scroll horizontal style={{ maxWidth: "100%" }}>
                <View style={{ padding: 14 }}>
                  <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-secondary-foreground">
                    {snippet()}
                  </Text>
                </View>
              </Scroll>
            </View>
          </If>
        </Section>
      </View>
    </Scroll>
  );
}
