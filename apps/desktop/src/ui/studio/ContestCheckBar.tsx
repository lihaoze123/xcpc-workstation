// The contest studio's check controls in the title bar (authoring-ui-refactor §4): the
// contest's status slot and one 「检查 ▾」 outline button. Progress stays inside the button
// (「检查中 2/4」, pressing it cancels) and in the outline marks, so nothing moves.

import { computed, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, tip } from "../kit";
import { AppMenu } from "../Menu";
import { Modal } from "../Modal";
import { compilerMissing, ensureToolchainLoaded, goSettings, type UiError } from "../store";
import {
  cancelContestCheck,
  contestBatch,
  contestCheck,
  exportCheckReport,
  idsToCheck,
  runTesterFolder,
  startContestCheck,
  testerResults,
  testerRunning,
} from "./contestCheck";
import { StatusSlot } from "./StatusSlot";
import { contestStatusModel } from "./statusLogic";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
// Shared by the two parts: the status slot sits after the contest name, the button on the right.
const notice = signal<UiError | undefined>(undefined);
const statusOpen = signal(false);
const folderIntroOpen = signal(false);

export function ContestCheckBar(props: {
  part: "status" | "button";
  contestId: string;
  readOnly: boolean;
  onGoProblem: (problemId: string) => void;
}) {
  ensureToolchainLoaded();

  const state = computed(() => (contestCheck()?.contestId === props.contestId ? contestCheck() : undefined));
  const batch = computed(() => {
    const b = contestBatch();
    return b?.running ? b : undefined;
  });

  const model = computed(() =>
    contestStatusModel(
      {
        report: state()?.report,
        // Progress lives in the button and the outline marks; the slot keeps the last summary.
        compilerMissing: compilerMissing(),
        error: notice() ?? state()?.error,
        tester: testerResults(),
      },
      { goProblem: props.onGoProblem, goSettings: () => goSettings("toolchain") },
    ),
  );

  function run(all: boolean, mode: "verify" | "sanitize" = "verify") {
    if (props.readOnly) return;
    if (compilerMissing()) {
      statusOpen.set(true);
      return;
    }
    notice.set(undefined);
    const report = state()?.report;
    let ids = idsToCheck(report, all || mode === "sanitize");
    if (ids.length === 0 && !all) ids = idsToCheck(report, true);
    const err = startContestCheck(props.contestId, ids, mode, () => statusOpen.set(true));
    if (err) notice.set(err);
  }

  async function tryTesterCode() {
    folderIntroOpen.set(false);
    if (compilerMissing()) {
      statusOpen.set(true);
      return;
    }
    const err = await runTesterFolder(props.contestId, props.readOnly, () => statusOpen.set(true));
    if (err) notice.set(err);
  }

  const W = 112;
  if (props.part === "status") {
    return (
      <StatusSlot
        testID="contest-status-slot"
        width={150}
        model={model()}
        open={statusOpen()}
        onOpenChange={(open) => {
          if (!open && notice()) notice.set(undefined);
          statusOpen.set(open);
        }}
      />
    );
  }
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
      <View style={{ width: W, flexDirection: "row", justifyContent: "flex-end" }}>
        <If
          when={!batch() && !testerRunning()}
          else={
            <Btn
              testID="contest-check-cancel"
              label={
                batch()
                  ? t("contestCheck.checking", { done: batch()!.completed, total: batch()!.total })
                  : t("build.external.running")
              }
              icon={batch() ? "x" : undefined}
              variant="outline"
              disabled={!batch()}
              tooltip={batch() ? tip(t("common.cancel")) : undefined}
              onPress={() => cancelContestCheck()}
              style={{ width: W }}
            />
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center" }}>
            <Btn
              testID="contest-check-btn"
              label={t("contestCheck.check")}
              variant="outline"
              disabled={props.readOnly}
              onPress={() => run(false)}
              style={{ width: W - 26, borderTopRightRadius: RADIUS.none, borderBottomRightRadius: RADIUS.none }}
            />
            <AppMenu
              align="end"
              minWidth={196}
              entries={[
                { id: "all", label: t("contestCheck.recheckAll"), disabled: props.readOnly },
                { id: "memory", label: t("contestCheck.memoryCheck"), disabled: props.readOnly },
                { id: "tester", label: t("contestCheck.tryTesterCode") },
                { separator: true, id: "sep" },
                { id: "export", label: t("contestCheck.exportReport"), disabled: !state()?.markdown },
              ]}
              onSelect={(id) => {
                if (id === "all") run(true);
                else if (id === "memory") run(true, "sanitize");
                else if (id === "tester") folderIntroOpen.set(true);
                else if (id === "export") {
                  void exportCheckReport(props.contestId).then((err) => {
                    if (err) notice.set(err);
                  });
                }
              }}
              trigger={
                <View
                  testID="contest-check-menu"
                  class="border border-l-0 border-border bg-card hover:bg-muted"
                  style={{
                    width: 26,
                    height: CONTROL.md,
                    borderTopRightRadius: RADIUS.sm,
                    borderBottomRightRadius: RADIUS.sm,
                    alignItems: "center",
                    justifyContent: "center",
                  }}
                >
                  <Icon name="chevronDown" size="sm" tone="secondary-foreground" />
                </View>
              }
            />
          </View>
        </If>
      </View>

      {/* One sentence before the folder picker: what the folder should look like (§4.1). */}
      <Modal open={folderIntroOpen()} onClose={() => folderIntroOpen.set(false)} width={400} testID="tester-folder-intro">
        <View style={{ padding: SPACE[16], gap: SPACE[16] }}>
          <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
            {t("contestCheck.tryTesterCode").replace("…", "")}
          </Text>
          <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.strong }} class="text-secondary-foreground">
            {t("contestCheck.folderIntro")}
          </Text>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
            <Btn label={t("common.cancel")} onPress={() => folderIntroOpen.set(false)} />
            <Btn testID="tester-folder-continue" label={t("contestCheck.folderContinue")} variant="primary" onPress={() => void tryTesterCode()} />
          </View>
        </View>
      </Modal>
    </View>
  );
}
