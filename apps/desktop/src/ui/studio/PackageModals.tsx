// Compact modals for structured package authoring (following design-system §9.1-6):
// - CreateSourceModal: new solution (`solutions/<name>.cpp`) or generator (`generators/<name>.cpp`)
// - AddGeneratorBatchModal: compact 4-field generator `[[test]]` batch creator
// - ConfirmDeleteRuleModal: confirm removing a `[[test]]` rule stating exact test count

import { effect, signal, Text, untracked, View } from "nsh";
import { Input } from "nsh/base";
import { Icon, type IconName } from "../Icon";
import { t } from "../i18n";
import { Btn, FieldLabel } from "../kit";
import { AppMenu } from "../Menu";
import { Modal } from "../Modal";
import { preview } from "../store";
import { MONO } from "../theme";
import { logicalSolutionName } from "./buildLogic";

const SAFE_STEM_RE = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/;

function ModalHeader(props: { icon: IconName; title: string; subtitle: string }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
      <View
        style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center", flexShrink: 0 }}
        class="bg-muted"
      >
        <Icon name={props.icon} size={16} tone="foreground" />
      </View>
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <Text style={{ fontSize: 14, fontWeight: 600 }} class="text-foreground">
          {props.title}
        </Text>
        <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
          {props.subtitle}
        </Text>
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
      <View style={{ padding: 18, gap: 14 }}>
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

        <View style={{ gap: 6 }}>
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
            style={{ fontSize: 12, numberOfLines: 1, textOverflow: "ellipsis" }}
            class={inlineError() ? "text-destructive" : "text-secondary-foreground"}
          >
            {inlineError() ?? t("build.programs.nameHint")}
          </Text>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
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
  onClose: () => void;
  onAdded: () => void;
}) {
  const genStems = () => props.generators.map(logicalSolutionName).filter(Boolean);
  const selectedGen = signal("");
  const args = signal("-n 100 {i}");
  const repeat = signal("5");
  const tag = signal("random");
  const inlineError = signal<string | undefined>(undefined);

  effect(() => {
    if (props.open) {
      untracked(() => {
        const stems = genStems();
        selectedGen.set(stems[0] ?? "");
        args.set("-n 100 {i}");
        repeat.set("5");
        tag.set("random");
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
    props.onClose();
    props.onAdded();
  }

  return (
    <Modal open={props.open} onClose={props.onClose} width={440} testID="add-gen-batch-modal">
      <View style={{ padding: 18, gap: 12 }}>
        <ModalHeader
          icon="sparkles"
          title={t("build.tests.genModalTitle")}
          subtitle={t("build.tests.genModalSubtitle")}
        />

        <View style={{ gap: 5 }}>
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
                  height: 32,
                  paddingLeft: 10,
                  paddingRight: 8,
                  borderRadius: 6,
                  flexDirection: "row",
                  alignItems: "center",
                  justifyContent: "space-between",
                }}
              >
                <Text style={{ fontSize: 13 }} class="text-foreground">
                  {selectedGen() || t("build.programs.generatorsEmpty")}
                </Text>
                <Icon name="chevronDown" size={14} tone="muted-foreground" />
              </View>
            }
          />
        </View>

        <View style={{ gap: 5 }}>
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

        <View style={{ flexDirection: "row", gap: 10 }}>
          <View style={{ flex: 1, gap: 5 }}>
            <FieldLabel label={t("build.tests.genModalRepeat")} />
            <Input
              value={repeat()}
              keyboardType="number-pad"
              onChange={(e) => repeat.set(e.value)}
              class="h-8 px-2.5 rounded-md border border-border bg-card text-[13px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ fontVariant: ["tabular-nums"] }}
            />
          </View>
          <View style={{ flex: 2, gap: 5 }}>
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

        <Text
          style={{ fontSize: 12, minHeight: 16 }}
          class={inlineError() ? "text-destructive" : "text-secondary-foreground"}
        >
          {inlineError() ?? t("build.tests.genModalHint")}
        </Text>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, paddingTop: 2 }}>
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
      <View style={{ padding: 18, gap: 12 }}>
        <ModalHeader
          icon="trash"
          title={
            props.testCount > 1
              ? t("build.tests.confirmDeleteBatchTitle", { count: props.testCount })
              : t("build.tests.confirmDeleteTitle")
          }
          subtitle={t("build.tests.confirmDeleteDesc")}
        />
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, paddingTop: 4 }}>
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
