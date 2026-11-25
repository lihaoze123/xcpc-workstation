// Single-File C++ Bundler Modal: pick a local .cpp file or paste C++ source, manage -I
// include dirs, toggle compact mode, inspect live bundled output, copy or save as.

import { Clipboard, effect, Scroll, signal, Text, View } from "nsh";
import { Editor, Switch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, createTransientFlag, IconBtn } from "../kit";
import { Modal } from "../Modal";
import { bundlerModalOpen, preview } from "../store";
import { MONO, monoFont } from "../theme";
import { parseJsonPayload } from "./snippetLogic";
import type { BundleResult } from "./types";

export function BundlerModal() {
  const mode = signal<"file" | "paste">("paste");
  const filePath = signal<string>("");
  const sourceText = signal<string>("");
  const includeDirs = signal<string[]>([]);
  const compact = signal(false);
  const result = signal<BundleResult | undefined>(undefined);
  const errorText = signal<string | undefined>(undefined);
  const copied = createTransientFlag(1600);
  const saved = createTransientFlag(1600);

  function runBundle() {
    const dirs = includeDirs();
    const isCompact = compact();
    if (mode() === "file" && filePath().trim()) {
      const res = preview.bundleLocalFileJson(filePath().trim(), dirs, isCompact);
      const parsed = parseJsonPayload<BundleResult>(res);
      if (parsed) {
        result.set(parsed);
        errorText.set(undefined);
      } else {
        result.set(undefined);
        errorText.set(res.error ?? t("snippets.error.bundleFailed"));
      }
      return;
    }
    const src = sourceText();
    if (!src.trim()) {
      result.set(undefined);
      errorText.set(undefined);
      return;
    }
    const res = preview.bundleLocalSourceJson(src, null, dirs, isCompact);
    const parsed = parseJsonPayload<BundleResult>(res);
    if (parsed) {
      result.set(parsed);
      errorText.set(undefined);
    } else {
      result.set(undefined);
      errorText.set(res.error ?? t("snippets.error.bundleFailed"));
    }
  }

  effect(() => {
    if (!bundlerModalOpen()) return;
    void mode();
    void filePath();
    void sourceText();
    void includeDirs();
    void compact();
    runBundle();
  });

  async function pickSourceFile() {
    const picked = await preview.pickCppFile({
      title: t("snippets.bundler.pickFile"),
    });
    if (picked.cancelled || picked.paths.length === 0) return;
    mode.set("file");
    filePath.set(picked.paths[0]);
  }

  async function addIncludeDir() {
    const picked = await preview.pickFolder({
      title: t("snippets.bundler.addIncludeDir"),
    });
    if (picked.cancelled || !picked.path) return;
    if (!includeDirs().includes(picked.path)) {
      includeDirs.set([...includeDirs(), picked.path]);
    }
  }

  function removeIncludeDir(dir: string) {
    includeDirs.set(includeDirs().filter((d) => d !== dir));
  }

  function copyOutput() {
    const code = result()?.code;
    if (!code) return;
    void Clipboard.writeText(code);
    copied.trigger();
  }

  async function saveOutput() {
    const code = result()?.code;
    if (!code) return;
    const res = await Preview.saveTextFileDialog(code, "bundled.cpp", {
      title: t("snippets.bundler.saveAs"),
    });
    if (res.ok) saved.trigger();
  }

  return (
    <Modal
      open={bundlerModalOpen()}
      onClose={() => bundlerModalOpen.set(false)}
      width={780}
      testID="bundler-modal"
    >
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View
            style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
            class="bg-muted"
          >
            <Icon name="fileCode2" size={16} tone="foreground" />
          </View>
          <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
            <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
              {t("snippets.bundler.title")}
            </Text>
            <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
              {t("snippets.bundler.desc")}
            </Text>
          </View>
          <IconBtn icon="x" size={26} iconSize={15} tone="muted-foreground" onPress={() => bundlerModalOpen.set(false)} />
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <Btn icon="folderOpen" label={t("snippets.bundler.pickFile")} onPress={() => void pickSourceFile()} />
          <Btn icon="folderPlus" label={t("snippets.bundler.addIncludeDir")} onPress={() => void addIncludeDir()} />
          <View style={{ flex: 1 }} />
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
            <Text style={{ fontSize: 12.5 }} class="text-secondary-foreground">
              {t("snippets.bundler.compact")}
            </Text>
            <Switch
              checked={compact()}
              onChange={(e) => compact.set(e.checked)}
              trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
              thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
            />
          </View>
        </View>

        <If when={includeDirs().length > 0}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <Each of={includeDirs()} key={(d) => d}>
              {(dir) => (
                <View
                  style={{
                    height: 24,
                    paddingLeft: 8,
                    paddingRight: 4,
                    borderRadius: 5,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 4,
                  }}
                  class="bg-muted"
                >
                  <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-secondary-foreground">
                    {dir()}
                  </Text>
                  <IconBtn icon="x" size={16} iconSize={11} onPress={() => removeIncludeDir(dir())} />
                </View>
              )}
            </Each>
          </View>
        </If>

        <If when={mode() === "file" && !!filePath()}>
          <View
            style={{
              height: 32,
              paddingLeft: 10,
              paddingRight: 6,
              borderRadius: 6,
              borderWidth: 1,
              flexDirection: "row",
              alignItems: "center",
              gap: 8,
            }}
            class="border-border bg-muted/50"
          >
            <Icon name="fileText" size={14} tone="secondary-foreground" />
            <Text
              style={{ flex: 1, fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
              class="text-foreground"
            >
              {filePath()}
            </Text>
            <IconBtn
              icon="x"
              size={20}
              iconSize={12}
              onPress={() => {
                filePath.set("");
                mode.set("paste");
              }}
            />
          </View>
        </If>

        <View style={{ flexDirection: "row", gap: 12, height: 340 }}>
          <If when={mode() === "paste"}>
            <View style={{ flex: 1, borderRadius: 8, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
              <Editor
                style={{ flex: 1 }}
                value={sourceText()}
                language="cpp"
                placeholder={t("snippets.bundler.inputPlaceholder")}
                fontFamily={monoFont()}
                fontSize={13}
                lineHeight={20}
                scrollBeyondLastLine={80}
                onChange={(e) => sourceText.set(e.value)}
                classes={{ gutter: "pl-2 pr-1", content: "pl-2" }}
              />
            </View>
          </If>

          <View style={{ flex: 1, borderRadius: 8, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
            <If
              when={!errorText()}
              else={
                <Scroll style={{ flex: 1 }} contentStyle={{ padding: 12 }}>
                  <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
                    <Icon name="circleAlert" size={15} tone="destructive" />
                    <Text style={{ flex: 1, fontSize: 13, lineHeight: 19, fontFamily: MONO }} class="text-destructive">
                      {errorText()}
                    </Text>
                  </View>
                </Scroll>
              }
            >
              <If
                when={Boolean(result()?.code)}
                else={
                  <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 16 }}>
                    <Text style={{ fontSize: 13, textAlign: "center" }} class="text-muted-foreground">
                      {t("snippets.bundler.outputPlaceholder")}
                    </Text>
                  </View>
                }
              >
                <Editor
                  style={{ flex: 1 }}
                  value={result()?.code ?? ""}
                  language="cpp"
                  readOnly
                  fontFamily={monoFont()}
                  fontSize={13}
                  lineHeight={20}
                  activeLine={false}
                  scrollBeyondLastLine={80}
                  classes={{ gutter: "pl-2 pr-1", content: "pl-2" }}
                />
              </If>
            </If>
          </View>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {result() ? t("snippets.bundler.inlinedCount", { count: (result()?.inlined_files ?? []).length }) : ""}
          </Text>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
            <Btn
              variant="outline"
              icon={saved.active() ? "check" : "fileDown"}
              label={saved.active() ? t("snippets.bundler.saved") : t("snippets.bundler.saveAs")}
              disabled={!result()?.code}
              onPress={() => void saveOutput()}
              style={{ minWidth: 92 }}
            />
            <Btn
              variant="primary"
              icon={copied.active() ? "check" : "copy"}
              label={copied.active() ? t("snippets.action.copied") : t("snippets.bundler.copyOutput")}
              disabled={!result()?.code}
              onPress={copyOutput}
              style={{ minWidth: 124 }}
            />
          </View>
        </View>
      </View>
    </Modal>
  );
}
