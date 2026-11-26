// Compact modals for structured package authoring (following design-system §9.1-6):
// - CreateSourceModal: new solution (`solutions/<name>.cpp`) or generator (`generators/<name>.cpp`)
// - AddGeneratorBatchModal: compact 4-field generator `[[test]]` batch creator
// - ConfirmDeleteRuleModal: confirm removing a `[[test]]` rule stating exact test count

import { effect, signal, Text, untracked, View } from "nsh";
import { Input, Switch } from "nsh/base";
import { Icon, type IconName } from "../Icon";
import { t } from "../i18n";
import { Btn, FieldLabel } from "../kit";
import { AppMenu } from "../Menu";
import { Modal } from "../Modal";
import { preview } from "../store";
import { MONO } from "../theme";
import {
  EXPECT_PRESETS,
  formatScoreExpect,
  logicalSolutionName,
  parseScoreExpectInput,
  type ScoreExpectData,
  type SubtaskScoringKind,
  type SubtaskSummaryData,
} from "./buildLogic";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
const SAFE_STEM_RE = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/;

function ModalHeader(props: { icon: IconName; title: string; subtitle?: string }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
      <View
        style={{ width: 32, height: CONTROL.lg, borderRadius: RADIUS.md, alignItems: "center", justifyContent: "center", flexShrink: 0 }}
        class="bg-muted"
      >
        <Icon name={props.icon} size="md" tone="foreground" />
      </View>
      <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
        <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
          {props.title}
        </Text>
        <If when={props.subtitle}>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
            {props.subtitle}
          </Text>
        </If>
      </View>
    </View>
  );
}

export function CreateSourceModal(props: {
  open: boolean;
  kind: "solution" | "generator";
  problemId: string;
  onClose: () => void;
  onCreated: (relPath: string) => void;
}) {
  const name = signal("");
  const inlineError = signal<string | undefined>(undefined);

  effect(() => {
    if (props.open) {
      untracked(() => {
        name.set("");
        inlineError.set(undefined);
      });
    }
  });

  function submit() {
    const cleaned = name().trim();
    if (!cleaned) return;
    if (!SAFE_STEM_RE.test(cleaned) || cleaned.includes("..")) {
      inlineError.set(t("build.programs.nameHint"));
      return;
    }
    const res = preview.createPackageSource(props.problemId, props.kind, cleaned);
    if (!res.ok) {
      inlineError.set(res.error ?? t("build.programs.nameHint"));
      return;
    }
    props.onClose();
    props.onCreated(res.relPath);
  }

  return (
    <Modal open={props.open} onClose={props.onClose} width={412} testID="create-source-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon={props.kind === "solution" ? "code" : "sparkles"}
          title={
            props.kind === "solution"
              ? t("build.programs.createSolutionTitle")
              : t("build.programs.createGeneratorTitle")
          }
          subtitle={
            props.kind === "solution"
              ? t("build.programs.createSolutionSubtitle")
              : t("build.programs.createGeneratorSubtitle")
          }
        />

        <View style={{ gap: SPACE[6] }}>
          <FieldLabel label={t("build.programs.nameLabel")} />
          <Input
            testID="create-source-name-input"
            value={name()}
            autoFocus
            autoCapitalize="none"
            autoCorrect={false}
            placeholder={
              props.kind === "solution"
                ? t("build.programs.solutionNamePlaceholder")
                : t("build.programs.generatorNamePlaceholder")
            }
            onChange={(e) => {
              name.set(e.value);
              if (inlineError()) inlineError.set(undefined);
            }}
            onSubmit={submit}
            class={
              inlineError()
                ? "h-8 px-2.5 rounded-md border border-destructive bg-card text-[13px] placeholder:text-muted-foreground focus:border-destructive focus:ring-1 focus:ring-inset focus:ring-destructive"
                : "h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
            }
            style={{ fontFamily: MONO }}
          />
          {/* Fixed single-slot constraint / error line — turns red in-place with zero dialog height jump */}
          <Text
            style={{ fontSize: FONT_SIZE.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
            class={inlineError() ? "text-destructive" : "text-secondary-foreground"}
          >
            {inlineError() ?? t("build.programs.nameHint")}
          </Text>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="create-source-submit"
            label={t("common.create")}
            variant="primary"
            disabled={!name().trim()}
            onPress={submit}
          />
        </View>
      </View>
    </Modal>
  );
}

export function AddGeneratorBatchModal(props: {
  open: boolean;
  problemId: string;
  generators: readonly string[];
  subtasks?: readonly SubtaskSummaryData[];
  initialGenerator?: string;
  onClose: () => void;
  onAdded: () => void;
}) {
  const genStems = () => props.generators.map(logicalSolutionName).filter(Boolean);
  const selectedGen = signal("");
  const args = signal("-n 100 {i}");
  const repeat = signal("5");
  const tag = signal("random");
  const selectedSubtask = signal<string | null>(null);
  const inlineError = signal<string | undefined>(undefined);

  effect(() => {
    if (props.open) {
      untracked(() => {
        const stems = genStems();
        const preferred = props.initialGenerator && stems.includes(props.initialGenerator) ? props.initialGenerator : stems[0] ?? "";
        selectedGen.set(preferred);
        args.set("-n 100 {i}");
        repeat.set("5");
        tag.set("random");
        selectedSubtask.set(props.subtasks?.[0]?.id ?? null);
        inlineError.set(undefined);
      });
    }
  });

  function submit() {
    const gen = selectedGen().trim();
    if (!gen) return;
    const rep = Math.max(1, Math.min(200, Number.parseInt(repeat().trim(), 10) || 1));
    const tg = tag().trim() || null;
    const res = preview.addGeneratorTest(props.problemId, gen, args().trim(), rep, tg, null);
    if (!res.ok) {
      inlineError.set(res.error ?? t("ctx.updatePackage"));
      return;
    }
    const sub = selectedSubtask();
    if (sub && (props.subtasks?.length ?? 0) > 0) {
      preview.setTestSubtask(props.problemId, res.ruleIndex, sub);
    }
    props.onClose();
    props.onAdded();
  }

  return (
    <Modal open={props.open} onClose={props.onClose} width={440} testID="add-gen-batch-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon="sparkles"
          title={t("build.tests.genModalTitle")}
          subtitle={t("build.tests.genModalSubtitle")}
        />

        <View style={{ gap: SPACE[4] }}>
          <FieldLabel label={t("build.tests.genModalGenerator")} />
          <AppMenu
            align="start"
            minWidth={220}
            entries={genStems().map((g) => ({
              id: g,
              label: g,
              checked: g === selectedGen(),
            }))}
            onSelect={(id) => {
              selectedGen.set(id);
              inlineError.set(undefined);
            }}
            trigger={
              <View
                class="border border-border bg-card hover:bg-muted"
                style={{
                  height: CONTROL.lg,
                  paddingLeft: SPACE[12],
                  paddingRight: SPACE[8],
                  borderRadius: RADIUS.sm,
                  flexDirection: "row",
                  alignItems: "center",
                  justifyContent: "space-between",
                }}
              >
                <Text style={{ fontSize: FONT_SIZE.body }} class="text-foreground">
                  {selectedGen() || t("build.programs.generatorsEmpty")}
                </Text>
                <Icon name="chevronDown" size="sm" tone="muted-foreground" />
              </View>
            }
          />
        </View>

        <View style={{ gap: SPACE[4] }}>
          <FieldLabel label={t("build.tests.genModalArgs")} />
          <Input
            value={args()}
            autoCapitalize="none"
            autoCorrect={false}
            placeholder="-n 200000 {i}"
            onChange={(e) => args.set(e.value)}
            class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
            style={{ fontFamily: MONO }}
          />
        </View>

        <View style={{ flexDirection: "row", gap: SPACE[8] }}>
          <View style={{ flex: 1, gap: SPACE[4] }}>
            <FieldLabel label={t("build.tests.genModalRepeat")} />
            <Input
              value={repeat()}
              keyboardType="number-pad"
              onChange={(e) => repeat.set(e.value)}
              class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ fontVariant: ["tabular-nums"] }}
            />
          </View>
          <View style={{ flex: 2, gap: SPACE[4] }}>
            <FieldLabel label={t("build.tests.genModalTag")} />
            <Input
              value={tag()}
              autoCapitalize="none"
              autoCorrect={false}
              placeholder="max / edge / random"
              onChange={(e) => tag.set(e.value)}
              class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
            />
          </View>
        </View>

        <If when={(props.subtasks?.length ?? 0) > 0}>
          <View style={{ gap: SPACE[4] }}>
            <FieldLabel label={t("build.subtasks.assignLabel")} />
            <AppMenu
              align="start"
              minWidth={220}
              entries={[
                { id: "__none__", label: t("build.subtasks.assignNone"), checked: selectedSubtask() == null },
                ...(props.subtasks ?? []).map((s) => ({
                  id: s.id,
                  label: `${t("build.table.subtaskHeader", { id: s.id })}  (${s.score})`,
                  checked: selectedSubtask() === s.id,
                })),
              ]}
              onSelect={(id) => selectedSubtask.set(id === "__none__" ? null : id)}
              trigger={
                <View
                  class="border border-border bg-card hover:bg-muted"
                  style={{
                    height: CONTROL.lg,
                    paddingLeft: SPACE[12],
                    paddingRight: SPACE[8],
                    borderRadius: RADIUS.sm,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                  }}
                >
                  <Text style={{ fontSize: FONT_SIZE.body }} class="text-foreground">
                    {selectedSubtask()
                      ? t("build.table.subtaskHeader", { id: selectedSubtask()! })
                      : t("build.subtasks.assignNone")}
                  </Text>
                  <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                </View>
              }
            />
          </View>
        </If>

        <Text
          style={{ fontSize: FONT_SIZE.caption, minHeight: 16 }}
          class={inlineError() ? "text-destructive" : "text-secondary-foreground"}
        >
          {inlineError() ?? t("build.tests.genModalHint")}
        </Text>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[2] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="add-gen-batch-submit"
            label={t("build.tests.add")}
            variant="primary"
            disabled={!selectedGen()}
            onPress={submit}
          />
        </View>
      </View>
    </Modal>
  );
}

export function ConfirmDeleteRuleModal(props: {
  open: boolean;
  testCount: number;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal open={props.open} onClose={props.onClose} width={390} testID="confirm-delete-rule-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon="trash"
          title={
            props.testCount > 1
              ? t("build.tests.confirmDeleteBatchTitle", { count: props.testCount })
              : t("build.tests.confirmDeleteTitle")
          }
          subtitle={t("build.tests.confirmDeleteDesc")}
        />
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[4] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="confirm-delete-rule-btn"
            label={
              props.testCount > 1
                ? t("build.tests.deleteBatchAction", { count: props.testCount })
                : t("build.tests.deleteRule")
            }
            variant="danger"
            onPress={props.onConfirm}
          />
        </View>
      </View>
    </Modal>
  );
}

export type PendingDeleteTarget =
  | { kind: "solution"; name: string }
  | { kind: "generator"; file: string; stem: string; refBatchCount: number }
  | { kind: "sample"; sampleIndex: number }
  | { kind: "selfTest"; relPath: string }
  | { kind: "interactor" }
  | { kind: "subtask"; id: string };

export function ConfirmPackageDeleteModal(props: {
  target: PendingDeleteTarget | undefined;
  onClose: () => void;
  onConfirm: (target: PendingDeleteTarget) => void;
}) {
  const title = () => {
    const cur = props.target;
    if (!cur) return "";
    if (cur.kind === "solution") return t("build.delete.solutionTitle", { name: cur.name });
    if (cur.kind === "generator") return t("build.delete.generatorTitle", { name: cur.stem });
    if (cur.kind === "sample") return t("build.delete.sampleTitle", { index: cur.sampleIndex });
    if (cur.kind === "interactor") return t("build.delete.interactorTitle");
    if (cur.kind === "subtask") return t("build.delete.subtaskTitle", { id: cur.id });
    return t("build.delete.selfTestTitle", { name: cur.relPath.split("/").slice(-2).join("/") });
  };

  const subtitle = () => {
    const cur = props.target;
    if (!cur) return "";
    if (cur.kind === "solution") return t("build.delete.solutionDesc");
    if (cur.kind === "generator") {
      return cur.refBatchCount > 0
        ? t("build.delete.generatorDescWithRefs", { count: cur.refBatchCount })
        : t("build.delete.generatorDesc");
    }
    if (cur.kind === "sample") return t("build.delete.sampleDesc");
    if (cur.kind === "interactor") return t("build.delete.interactorDesc");
    if (cur.kind === "subtask") return t("build.delete.subtaskDesc");
    return t("build.delete.selfTestDesc");
  };

  return (
    <Modal open={props.target !== undefined} onClose={props.onClose} width={400} testID="confirm-package-delete-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader icon="trash" title={title()} subtitle={subtitle()} />
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[2] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="confirm-package-delete-btn"
            label={t("common.delete")}
            variant="danger"
            onPress={() => {
              if (props.target) props.onConfirm(props.target);
            }}
          />
        </View>
      </View>
    </Modal>
  );
}

/** Add or edit an OI `[[subtask]]` entry (`id`, `score`, `scoring`, `depends`). */
export function SubtaskModal(props: {
  open: boolean;
  problemId: string;
  editing?: SubtaskSummaryData;
  existingSubtasks: readonly SubtaskSummaryData[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const id = signal("");
  const score = signal("30");
  const scoring = signal<SubtaskScoringKind>("min");
  const depends = signal<string[]>([]);
  const inlineError = signal<string | undefined>(undefined);

  effect(() => {
    if (props.open) {
      untracked(() => {
        if (props.editing) {
          id.set(props.editing.id);
          score.set(String(props.editing.score));
          scoring.set(props.editing.scoring);
          depends.set([...(props.editing.depends ?? [])]);
        } else {
          const nextNum = props.existingSubtasks.length + 1;
          id.set(String(nextNum));
          score.set(nextNum === 1 ? "30" : "70");
          scoring.set("min");
          depends.set([]);
        }
        inlineError.set(undefined);
      });
    }
  });

  const depCandidates = () =>
    props.existingSubtasks.filter((s) => s.id !== (props.editing?.id ?? id().trim()));

  function toggleDep(depId: string) {
    const cur = depends();
    if (cur.includes(depId)) {
      depends.set(cur.filter((x) => x !== depId));
    } else {
      depends.set([...cur, depId]);
    }
  }

  function submit() {
    const cleanedId = id().trim();
    if (!cleanedId || !SAFE_STEM_RE.test(cleanedId) || cleanedId.includes("..")) {
      inlineError.set(t("build.programs.nameHint"));
      return;
    }
    const parsedScore = Number.parseInt(score().trim(), 10);
    if (!Number.isFinite(parsedScore) || parsedScore < 0) {
      inlineError.set(t("build.scoring.scoreError"));
      return;
    }
    const res = preview.upsertSubtask(
      props.problemId,
      cleanedId,
      parsedScore,
      depends(),
      scoring(),
    );
    if (!res.ok) {
      inlineError.set(res.error ?? t("ctx.updatePackage"));
      return;
    }
    props.onClose();
    props.onSaved();
  }

  return (
    <Modal open={props.open} onClose={props.onClose} width={430} testID="subtask-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon="listOrdered"
          title={
            props.editing
              ? t("build.subtasks.modalEditTitle", { id: props.editing.id })
              : t("build.subtasks.modalAddTitle")
          }
        />

        <View style={{ flexDirection: "row", gap: SPACE[8] }}>
          <View style={{ flex: 1, gap: SPACE[4] }}>
            <FieldLabel label={t("build.subtasks.idLabel")} />
            <Input
              testID="subtask-id-input"
              value={id()}
              readonly={Boolean(props.editing)}
              autoFocus={!props.editing}
              autoCapitalize="none"
              autoCorrect={false}
              placeholder={t("build.subtasks.idPlaceholder")}
              onChange={(e) => {
                id.set(e.value);
                if (inlineError()) inlineError.set(undefined);
              }}
              onSubmit={submit}
              class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ fontFamily: MONO, opacity: props.editing ? 0.65 : 1 }}
            />
          </View>
          <View style={{ width: 112, gap: SPACE[4] }}>
            <FieldLabel label={t("build.subtasks.scoreLabel")} />
            <Input
              testID="subtask-score-input"
              value={score()}
              autoFocus={Boolean(props.editing)}
              keyboardType="number-pad"
              onChange={(e) => {
                score.set(e.value);
                if (inlineError()) inlineError.set(undefined);
              }}
              onSubmit={submit}
              class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ fontVariant: ["tabular-nums"] }}
            />
          </View>
        </View>

        <View style={{ gap: SPACE[4] }}>
          <FieldLabel label={t("build.subtasks.scoringLabel")} />
          <View style={{ flexDirection: "row", gap: SPACE[8] }}>
            {(["min", "sum"] as const).map((mode) => (
              <View
                onPress={() => scoring.set(mode)}
                class={scoring() === mode ? "bg-accent border border-border" : "border border-border hover:bg-muted"}
                style={{
                  flex: 1,
                  height: CONTROL.md,
                  borderRadius: RADIUS.sm,
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                <Text
                  style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                  class={scoring() === mode ? "text-foreground" : "text-secondary-foreground"}
                >
                  {mode === "min" ? t("build.subtasks.scoringMin") : t("build.subtasks.scoringSum")}
                </Text>
              </View>
            ))}
          </View>
        </View>

        <If when={depCandidates().length > 0}>
          <View style={{ gap: SPACE[6] }}>
            <FieldLabel label={t("build.subtasks.dependsLabel")} />
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[6] }}>
              <Each of={depCandidates()} key={(s) => s.id}>
                {(s) => {
                  const active = () => depends().includes(s().id);
                  return (
                    <View
                      onPress={() => toggleDep(s().id)}
                      class={active() ? "bg-accent border border-border" : "border border-border hover:bg-muted"}
                      style={{ height: CONTROL.sm, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.sm, justifyContent: "center" }}
                    >
                      <Text
                        style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
                        class={active() ? "text-foreground" : "text-secondary-foreground"}
                      >
                        {t("build.table.subtaskHeader", { id: s().id })}
                      </Text>
                    </View>
                  );
                }}
              </Each>
            </View>
          </View>
        </If>

        <If when={inlineError()}>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-destructive">
            {inlineError()!}
          </Text>
        </If>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[2] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="subtask-modal-submit"
            label={t("build.subtasks.save")}
            variant="primary"
            disabled={!id().trim()}
            onPress={submit}
          />
        </View>
      </View>
    </Modal>
  );
}

/** Edit `[solutions.<name>]` OI scoring assertions (`score` and per-subtask verdicts). */
export function SolutionScoringModal(props: {
  open: boolean;
  problemId: string;
  solutionName: string;
  initialScore?: ScoreExpectData | null;
  initialSubtasks?: Record<string, string | string[]>;
  subtasks: readonly SubtaskSummaryData[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const scoreInput = signal("");
  const subMap = signal<Record<string, string[]>>({});
  const inlineError = signal<string | undefined>(undefined);

  effect(() => {
    if (props.open) {
      untracked(() => {
        scoreInput.set(formatScoreExpect(props.initialScore)?.replace("–", "-") ?? "");
        const next: Record<string, string[]> = {};
        for (const [k, v] of Object.entries(props.initialSubtasks ?? {})) {
          next[k] = Array.isArray(v) ? [...v] : [v];
        }
        subMap.set(next);
        inlineError.set(undefined);
      });
    }
  });

  function setSubtaskVerdict(sid: string, verdicts: string[] | null) {
    const next = { ...subMap() };
    if (!verdicts || verdicts.length === 0) {
      delete next[sid];
    } else {
      next[sid] = verdicts;
    }
    subMap.set(next);
  }

  function submit() {
    const parsed = parseScoreExpectInput(scoreInput());
    if (!parsed.ok) {
      inlineError.set(t("build.scoring.scoreError"));
      return;
    }
    const scoreJson = parsed.value == null ? "null" : JSON.stringify(parsed.value);
    const outSubtasks: Record<string, string | string[]> = {};
    for (const [sid, list] of Object.entries(subMap())) {
      if (list.length === 1) outSubtasks[sid] = list[0]!;
      else if (list.length > 1) outSubtasks[sid] = list;
    }
    const res = preview.setSolutionScoring(
      props.problemId,
      props.solutionName,
      scoreJson,
      JSON.stringify(outSubtasks),
    );
    if (!res.ok) {
      inlineError.set(res.error ?? t("ctx.updatePackage"));
      return;
    }
    props.onClose();
    props.onSaved();
  }

  return (
    <Modal open={props.open} onClose={props.onClose} width={450} testID="solution-scoring-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon="scale"
          title={t("build.scoring.modalTitle", { name: props.solutionName })}
          subtitle={t("build.scoring.modalSubtitle")}
        />

        <View style={{ gap: SPACE[4] }}>
          <FieldLabel label={t("build.scoring.scoreLabel")} />
          <Input
            testID="solution-score-expect-input"
            value={scoreInput()}
            autoFocus
            autoCapitalize="none"
            autoCorrect={false}
            placeholder={t("build.scoring.scorePlaceholder")}
            onChange={(e) => {
              scoreInput.set(e.value);
              if (inlineError()) inlineError.set(undefined);
            }}
            onSubmit={submit}
            class={
              inlineError()
                ? "h-8 px-2.5 rounded-md border border-destructive bg-card text-[13px] placeholder:text-muted-foreground focus:border-destructive focus:ring-1 focus:ring-inset focus:ring-destructive"
                : "h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
            }
            style={{ fontVariant: ["tabular-nums"] }}
          />
          <Text
            style={{ fontSize: FONT_SIZE.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
            class={inlineError() ? "text-destructive" : "text-secondary-foreground"}
          >
            {inlineError() ?? t("build.scoring.scoreHint")}
          </Text>
        </View>

        <If when={props.subtasks.length > 0}>
          <View style={{ gap: SPACE[6] }}>
            <FieldLabel label={t("build.scoring.subtasksLabel")} />
            <View style={{ gap: SPACE[6] }}>
              <Each of={props.subtasks} key={(s) => s.id}>
                {(s) => {
                  const cur = () => subMap()[s().id];
                  const curLabel = () => (cur() && cur()!.length > 0 ? cur()!.join(" / ") : t("build.scoring.anyVerdict"));
                  return (
                    <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
                      <Text style={{ fontSize: FONT_SIZE.body, fontVariant: ["tabular-nums"] }} class="text-foreground">
                        {`${t("build.table.subtaskHeader", { id: s().id })}  (${s().score})`}
                      </Text>
                      <AppMenu
                        align="end"
                        minWidth={150}
                        entries={[
                          { id: "__any__", label: t("build.scoring.anyVerdict"), checked: !cur() },
                          ...EXPECT_PRESETS.map((preset) => ({
                            id: preset.join(","),
                            label: preset.join(" / "),
                            checked: cur()?.join(",") === preset.join(","),
                          })),
                        ]}
                        onSelect={(id) => {
                          if (id === "__any__") setSubtaskVerdict(s().id, null);
                          else setSubtaskVerdict(s().id, id.split(","));
                        }}
                        trigger={
                          <View
                            class="border border-border bg-card hover:bg-muted"
                            style={{
                              height: CONTROL.md,
                              minWidth: 112,
                              paddingLeft: SPACE[8],
                              paddingRight: SPACE[8],
                              borderRadius: RADIUS.sm,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: SPACE[6],
                            }}
                          >
                            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                              {curLabel()}
                            </Text>
                            <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                          </View>
                        }
                      />
                    </View>
                  );
                }}
              </Each>
            </View>
          </View>
        </If>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[2] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="solution-scoring-submit"
            label={t("build.subtasks.save")}
            variant="primary"
            onPress={submit}
          />
        </View>
      </View>
    </Modal>
  );
}

/** Package scaffold initialization modal allowing optional Phase 3C features. */
export function InitPackageModal(props: {
  open: boolean;
  onClose: () => void;
  onConfirm: (opts: { interactive: boolean; runs: number | null; subtasks: boolean }) => void;
}) {
  const interactive = signal(false);
  const subtasks = signal(false);
  const runs = signal(1);

  effect(() => {
    if (props.open) {
      untracked(() => {
        interactive.set(false);
        subtasks.set(false);
        runs.set(1);
      });
    }
  });

  return (
    <Modal open={props.open} onClose={props.onClose} width={430} testID="init-package-modal">
      <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
        <ModalHeader
          icon="database"
          title={t("build.initModal.title")}
          subtitle={t("build.initModal.subtitle")}
        />

        <View style={{ gap: SPACE[8] }}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[12] }}>
            <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
              <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("build.initModal.interactive")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                {t("build.initModal.interactiveDesc")}
              </Text>
            </View>
            <Switch checked={interactive()} onChange={(e) => interactive.set(e.checked)} />
          </View>

          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[12] }}>
            <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
              <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("build.initModal.subtasks")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                {t("build.initModal.subtasksDesc")}
              </Text>
            </View>
            <Switch checked={subtasks()} onChange={(e) => subtasks.set(e.checked)} />
          </View>

          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[12] }}>
            <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("build.initModal.runs")}
            </Text>
            <AppMenu
              align="end"
              minWidth={150}
              entries={[1, 2, 3, 4].map((n) => ({
                id: String(n),
                label: n === 1 ? t("build.interactor.runsSingle") : t("build.interactor.runsMulti", { count: n }),
                checked: runs() === n,
              }))}
              onSelect={(id) => runs.set(Number.parseInt(id, 10) || 1)}
              trigger={
                <View
                  class="border border-border bg-card hover:bg-muted"
                  style={{
                    height: CONTROL.md,
                    paddingLeft: SPACE[8],
                    paddingRight: SPACE[8],
                    borderRadius: RADIUS.sm,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[6],
                  }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                    {runs() === 1 ? t("build.interactor.runsSingle") : t("build.interactor.runsMulti", { count: runs() })}
                  </Text>
                  <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                </View>
              }
            />
          </View>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8], paddingTop: SPACE[2] }}>
          <Btn label={t("common.cancel")} onPress={props.onClose} />
          <Btn
            testID="init-package-modal-confirm"
            label={t("build.initModal.confirm")}
            variant="primary"
            onPress={() => {
              props.onClose();
              props.onConfirm({
                interactive: interactive(),
                runs: runs() >= 2 ? runs() : null,
                subtasks: subtasks(),
              });
            }}
          />
        </View>
      </View>
    </Modal>
  );
}
