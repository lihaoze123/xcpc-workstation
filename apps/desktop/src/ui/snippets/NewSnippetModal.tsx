// Creation modals for Algorithm Snippets (supporting pre-filled code from Library/Editorial)
// and Living TRD Handbooks.

import { effect, signal, Text, View } from "nsh";
import { Input, Switch } from "nsh/base";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, IconBtn } from "../kit";
import { Modal } from "../Modal";
import {
  bumpSnippetsRevision,
  newSnippetSeed,
  newTrdModalOpen,
  openSnippetStudio,
  openTrdStudio,
  preview,
  selectedSnippetSlug,
  selectedTrdId,
} from "../store";
import { MONO } from "../theme";
import { encodeStringPatch, suggestSnippetSlug } from "./snippetLogic";

export function NewSnippetModal() {
  const slug = signal("");
  const slugTouched = signal(false);
  const title = signal("");
  const formulaOnly = signal(false);
  const sourceProblem = signal("");
  const seedCode = signal<string | undefined>(undefined);
  const error = signal<string | undefined>(undefined);

  effect(() => {
    const seed = newSnippetSeed();
    if (!seed) return;
    // Per review 2.6: when promoting from a problem/editorial, avoid pre-filling raw Chinese
    // problem title that produces a generic `snippet` slug; let user name the algorithm cleanly.
    const fromPromote = Boolean(seed.code);
    title.set(fromPromote ? "" : (seed.title ?? ""));
    slug.set(!fromPromote && seed.title ? suggestSnippetSlug(seed.title) : "");
    slugTouched.set(false);
    formulaOnly.set(false);
    sourceProblem.set(seed.sourceProblem ?? "");
    seedCode.set(seed.code);
    error.set(undefined);
  });

  function close() {
    newSnippetSeed.set(undefined);
  }

  function submit() {
    const cleanSlug = slug().trim();
    const cleanTitle = title().trim();
    if (!cleanSlug || !cleanTitle) {
      error.set(t("snippets.newModal.slugHint"));
      return;
    }
    const meta = {
      title: cleanTitle,
      tags: [],
      time_complexity: null,
      space_complexity: null,
      source_problem: sourceProblem().trim() || null,
      verify: [],
    };
    const code = formulaOnly()
      ? null
      : (seedCode() ?? "#pragma once\n#include <bits/stdc++.h>\nusing namespace std;\n");
    const notes = formulaOnly() ? "" : "";
    const res = preview.createSnippet(cleanSlug, JSON.stringify(meta), code, notes);
    if (!res.ok) {
      error.set(res.error ?? t("snippets.error.createFailed"));
      return;
    }
    bumpSnippetsRevision();
    selectedSnippetSlug.set(cleanSlug);
    close();
    openSnippetStudio(cleanSlug);
  }

  const importedLines = () => (seedCode() ? seedCode()!.split("\n").length : 0);
  const importedPreview = () => (seedCode() ? seedCode()!.split("\n").slice(0, 6).join("\n") : "");

  return (
    <Modal open={newSnippetSeed() !== undefined} onClose={close} width={460} testID="new-snippet-modal">
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View
            style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
            class="bg-muted"
          >
            <Icon name="code" size={16} tone="foreground" />
          </View>
          <View style={{ flex: 1, gap: 2 }}>
            <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
              {t("snippets.newModal.title")}
            </Text>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.newModal.desc")}
            </Text>
          </View>
          <IconBtn icon="x" size={26} iconSize={15} tone="muted-foreground" onPress={close} />
        </View>

        <View style={{ gap: 12 }}>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.newModal.name")}
            </Text>
            <Input
              value={title()}
              autoFocus
              onChange={(e) => {
                title.set(e.value);
                if (!slugTouched()) slug.set(suggestSnippetSlug(e.value));
              }}
              onSubmit={submit}
              class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
            />
          </View>

          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.newModal.slug")}
            </Text>
            <Input
              value={slug()}
              onChange={(e) => {
                slugTouched.set(true);
                slug.set(e.value);
                error.set(undefined);
              }}
              onSubmit={submit}
              class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13, fontFamily: MONO }}
            />
            <Text
              style={{ fontSize: 12, lineHeight: 17 }}
              class={error() ? "text-destructive" : "text-muted-foreground"}
            >
              {error() ?? t("snippets.newModal.slugHint")}
            </Text>
          </View>

          <If when={Boolean(seedCode())}>
            <View style={{ gap: 4 }}>
              <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {t("snippets.newModal.importedPreview", { lines: importedLines() })}
              </Text>
              <View
                style={{ padding: 8, borderRadius: 6, borderWidth: 1, maxHeight: 112, overflow: "hidden" }}
                class="border-border bg-muted/60"
              >
                <Text style={{ fontSize: 11.5, lineHeight: 16, fontFamily: MONO }} class="text-secondary-foreground">
                  {importedPreview()}
                </Text>
              </View>
            </View>
          </If>

          <If when={!seedCode()}>
            <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", paddingTop: 2 }}>
              <Text style={{ fontSize: 13 }} class="text-foreground">
                {t("snippets.newModal.formulaOnly")}
              </Text>
              <Switch
                checked={formulaOnly()}
                onChange={(e) => formulaOnly.set(e.checked)}
                trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
                thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
              />
            </View>
          </If>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, paddingTop: 4 }}>
          <Btn variant="outline" label={t("common.cancel")} onPress={close} />
          <Btn variant="primary" label={t("snippets.newModal.create")} onPress={submit} />
        </View>
      </View>
    </Modal>
  );
}

export function NewTrdModal() {
  const id = signal("");
  const idTouched = signal(false);
  const name = signal("");
  const author = signal("");
  const error = signal<string | undefined>(undefined);

  effect(() => {
    if (!newTrdModalOpen()) return;
    id.set("");
    idTouched.set(false);
    name.set("");
    author.set("");
    error.set(undefined);
  });

  function close() {
    newTrdModalOpen.set(false);
  }

  function submit() {
    const cleanId = id().trim();
    const cleanName = name().trim();
    if (!cleanId || !cleanName) {
      error.set(t("snippets.newModal.slugHint"));
      return;
    }
    const patch = {
      name: cleanName,
      author: encodeStringPatch(author()),
      template: encodeStringPatch("trd-compact"),
    };
    const res = preview.composeTrd(cleanId, JSON.stringify([]), JSON.stringify(patch));
    if (!res.ok) {
      error.set(res.error ?? t("snippets.error.createFailed"));
      return;
    }
    bumpSnippetsRevision();
    selectedTrdId.set(cleanId);
    close();
    openTrdStudio(cleanId);
  }

  return (
    <Modal open={newTrdModalOpen()} onClose={close} width={440} testID="new-trd-modal">
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View
            style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
            class="bg-muted"
          >
            <Icon name="bookOpen" size={16} tone="foreground" />
          </View>
          <View style={{ flex: 1, gap: 2 }}>
            <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
              {t("snippets.trd.newTitle")}
            </Text>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.trd.newDesc")}
            </Text>
          </View>
          <IconBtn icon="x" size={26} iconSize={15} tone="muted-foreground" onPress={close} />
        </View>

        <View style={{ gap: 12 }}>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.trd.nameLabel")}
            </Text>
            <Input
              value={name()}
              autoFocus
              onChange={(e) => {
                name.set(e.value);
                if (!idTouched()) id.set(suggestSnippetSlug(e.value));
              }}
              onSubmit={submit}
              class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
            />
          </View>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.trd.idLabel")}
            </Text>
            <Input
              value={id()}
              onChange={(e) => {
                idTouched.set(true);
                id.set(e.value);
                error.set(undefined);
              }}
              onSubmit={submit}
              class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13, fontFamily: MONO }}
            />
            <Text
              style={{ fontSize: 12, lineHeight: 17 }}
              class={error() ? "text-destructive" : "text-muted-foreground"}
            >
              {error() ?? t("snippets.newModal.slugHint")}
            </Text>
          </View>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
              {t("snippets.trd.authorLabel")}
            </Text>
            <Input
              value={author()}
              onChange={(e) => author.set(e.value)}
              onSubmit={submit}
              class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
              style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
            />
          </View>
        </View>

        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, paddingTop: 4 }}>
          <Btn variant="outline" label={t("common.cancel")} onPress={close} />
          <Btn variant="primary" label={t("snippets.newModal.create")} onPress={submit} />
        </View>
      </View>
    </Modal>
  );
}
