// Shared trust-confirmation modal for third-party plugins requesting network domains,
// workspace read access, or credential storage. Used by Settings -> Plugins, ToolsView,
// and PluginRepoBrowserModal so installing a permissioned plugin never looks silently stuck.

import { Text, View } from "nsh";
import type { PluginSummaryJs } from "xcpc-workstation/native";
import { Btn } from "./kit";
import { DialogHeader, Modal } from "./Modal";
import { MONO } from "./theme";
import { zhEn } from "./training/locale";
import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";

export function PluginTrustModal(props: {
  plugin: PluginSummaryJs | undefined;
  onConfirm: (plugin: PluginSummaryJs) => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      open={props.plugin !== undefined}
      onClose={props.onCancel}
      width={460}
      testID="plugin-trust-modal"
    >
      <If when={props.plugin}>
        {(plugin) => (
          <View style={{ padding: SPACE[20], gap: SPACE[16] }}>
            <DialogHeader
              icon="triangleAlert"
              tone="foreground"
              title={zhEn(
                `信任并启用「${plugin().name}」？`,
                `Trust and enable "${plugin().name}"?`,
              )}
              description={zhEn(
                "该插件在启用前需要你的明确确认，请核对以下权限：",
                "Review the permissions requested by this plugin before enabling it:",
              )}
            />

            <View style={{ gap: SPACE[8] }}>
              <If when={plugin().domains.length > 0}>
                <View
                  testID="plugin-trust-domains"
                  class="bg-muted/60 border border-border"
                  style={{ borderRadius: RADIUS.md, padding: SPACE[12], gap: SPACE[6] }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
                    {zhEn("请求访问的网络域名", "Requested network domains")}
                  </Text>
                  <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[6] }}>
                    <Each of={plugin().domains} key={(d) => d}>
                      {(domain) => (
                        <View
                          class="bg-card border border-border"
                          style={{ borderRadius: RADIUS.xs, paddingLeft: SPACE[8], paddingRight: SPACE[8], height: 22, justifyContent: "center" }} // token-exempt: compact status badge height
                        >
                          <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO }} class="text-foreground">
                            {domain()}
                          </Text>
                        </View>
                      )}
                    </Each>
                  </View>
                </View>
              </If>

              <If when={plugin().workspaceRead}>
                <View
                  class="bg-muted/60 border border-border"
                  style={{ borderRadius: RADIUS.md, padding: SPACE[12] }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-secondary-foreground">
                    {zhEn("请求只读访问当前工作区文件。", "Requests read-only access to the current workspace.")}
                  </Text>
                </View>
              </If>

              <If when={Boolean(plugin().workspaceWrite)}>
                <View
                  testID="plugin-trust-workspace-write"
                  class="bg-muted/60 border border-destructive/40"
                  style={{ borderRadius: RADIUS.md, padding: SPACE[12] }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-destructive">
                    {zhEn(
                      "请求向当前工作区写入题目、样例与算法模板（不可删除现有文件或修改冻结比赛）。",
                      "Requests write access to create or update problems, samples, and snippets in the current workspace (cannot delete files or modify frozen contests).",
                    )}
                  </Text>
                </View>
              </If>

              <If when={plugin().credentials.length > 0}>
                <View
                  class="bg-muted/60 border border-border"
                  style={{ borderRadius: RADIUS.md, padding: SPACE[12] }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-secondary-foreground">
                    {zhEn(
                      `请求读写凭据：${plugin().credentials.map((c) => c.label || c.name).join("、")}`,
                      `Requests credential access: ${plugin().credentials.map((c) => c.label || c.name).join(", ")}`,
                    )}
                  </Text>
                </View>
              </If>
            </View>

            <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
              <Btn
                testID="plugin-trust-cancel"
                label={zhEn("取消", "Cancel")}
                variant="outline"
                onPress={props.onCancel}
              />
              <Btn
                testID="plugin-trust-confirm"
                label={zhEn("信任并启用", "Trust & Enable")}
                variant="primary"
                onPress={() => props.onConfirm(plugin())}
              />
            </View>
          </View>
        )}
      </If>
    </Modal>
  );
}
