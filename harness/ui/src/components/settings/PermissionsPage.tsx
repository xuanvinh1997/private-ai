import { createResource, For, Show } from "solid-js";
import { inTauri } from "../../lib/agent";
import { S, t, tn, TRich, type Msg } from "../../lib/i18n";
import { defaultToolScope, setDefaultToolScope } from "../../lib/prefs";
import type { ToolScope } from "../../lib/protocol";
import { Disclosure } from "../primitives";
import { Banner, Row, RowGroup, SectionHead, Select } from "./FormKit";
import { describeHarness, docCayPlugin, sandboxStatus } from "./harness";

/** The Permissions page, in two sections: the upper one is editable because scope is the user's decision, the lower is read-only because confinement is a fact about the machine. */

const NHAN: Record<ToolScope, Msg> = {
  read: S.settings.perms.scopeRead,
  write: S.settings.perms.scopeWrite,
  shell: S.settings.perms.scopeShell,
};

/** What each scope means, saying what opens and then what stays closed, so it reads as a choice. */
const HAU_QUA: Record<ToolScope, Msg> = {
  read: S.settings.perms.scopeReadDesc,
  write: S.settings.perms.scopeWriteDesc,
  shell: S.settings.perms.scopeShellDesc,
};

/** The same three sentences in full, kept in an `InfoDot` rather than spread on the page. */
const HAU_QUA_DAI: Record<ToolScope, Msg> = {
  read: S.settings.perms.scopeReadMore,
  write: S.settings.perms.scopeWriteMore,
  shell: S.settings.perms.scopeShellMore,
};

/** Whether the `sandbox` row is mounted, with a colour so it can be skimmed. */
type TrangThai = { text: Msg; tone: "faint" | "ok" | "warn" | "danger" };

/** Confinement level labels: three levels, three different sentences, never merged. */
const MUC_GIAM: Record<string, { nhan: Msg; tone: "ok" | "warn" | "danger" }> = {
  full: { nhan: S.settings.perms.levelFull, tone: "ok" },
  partial: { nhan: S.settings.perms.levelPartial, tone: "warn" },
  none: { nhan: S.settings.perms.levelNone, tone: "danger" },
};

export default function PermissionsPage() {
  // Ask the core for the real level; `null` means unanswerable, which is not the same as `none`.
  const [giam] = createResource(sandboxStatus);

  // The plugin tree answers one question, and is re-asked per visit since it follows the project.
  const [cay] = createResource(async () => docCayPlugin(await describeHarness()));

  const sandbox = (): TrangThai => {
    if (!inTauri()) return { text: S.settings.perms.pluggedDemo, tone: "faint" };
    if (cay.loading) return { text: S.settings.perms.pluggedAsking, tone: "faint" };
    if (cay.error !== undefined) return { text: S.settings.perms.pluggedError, tone: "danger" };
    const row = cay()?.find((item) => item.id === "sandbox");
    if (row === undefined) return { text: S.settings.perms.pluggedMissing, tone: "warn" };
    if (row.disabled) return { text: S.settings.perms.pluggedOff, tone: "warn" };
    return { text: S.settings.perms.pluggedYes, tone: "ok" };
  };

  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          icon="hand"
          title={t(S.settings.perms.defaultTitle)}
          desc={t(S.settings.perms.defaultDesc)}
        />
        <RowGroup>
          <Row
            icon="shield"
            label={t(S.settings.perms.scopeRow)}
            desc={t(HAU_QUA[defaultToolScope()])}
            more={t(HAU_QUA_DAI[defaultToolScope()])}
            control={() => (
              <Select
                label={t(S.settings.perms.scopeSelect)}
                value={defaultToolScope()}
                onPick={(value) => setDefaultToolScope(value as ToolScope)}
                options={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
                  id: scope,
                  label: t(NHAN[scope]),
                }))}
              />
            )}
            below={() => (
              <Show when={defaultToolScope() === "shell"}>
                <Banner
                  tone="warn"
                  icon="warn"
                  title={t(S.settings.perms.shellWarnTitle)}
                  more={t(S.settings.perms.shellWarnMore)}
                >
                  {/* One whole sentence, never assembled from fragments: clause order differs per language. */}
                  <b>
                    <TRich msg={S.settings.perms.shellWarnBody} />
                  </b>
                </Banner>
              </Show>
            )}
          />
          <Row
            icon="pencil"
            label={t(S.settings.perms.composerPicker)}
            desc={t(S.settings.perms.composerPickerDesc)}
            more={t(S.settings.perms.composerPickerMore)}
          />
        </RowGroup>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          icon="shield"
          title={t(S.settings.perms.sandboxTitle)}
          desc={t(S.settings.perms.sandboxDesc)}
          more={t(S.settings.perms.sandboxMore)}
        />

        <RowGroup>
          <Row
            icon="monitor"
            label={t(S.settings.perms.levelRow)}
            desc={giam()?.reason ?? t(S.settings.perms.levelDesc)}
            more={t(S.settings.perms.levelMore)}
            control={() => (
              <Show
                when={giam()}
                fallback={
                  <span class="text-2xs text-faint">{t(S.settings.perms.levelUnknown)}</span>
                }
              >
                {(muc) => (
                  <span
                    class="text-2xs font-medium"
                    classList={{
                      "text-ok": MUC_GIAM[muc().mode]?.tone === "ok",
                      "text-warn": MUC_GIAM[muc().mode]?.tone === "warn",
                      "text-danger": MUC_GIAM[muc().mode]?.tone === "danger",
                    }}
                  >
                    {(() => {
                      const muc_giam = MUC_GIAM[muc().mode];
                      return muc_giam === undefined ? muc().mode : t(muc_giam.nhan);
                    })()}
                  </span>
                )}
              </Show>
            )}
          />
          <Show when={giam()?.writableRoots.length}>
            <Row
              icon="folder"
              label={t(S.settings.perms.rootsRow)}
              desc={t(S.settings.perms.rootsDesc)}
              more={t(S.settings.perms.rootsMore)}
              below={() => (
                <ul class="m-0 flex list-none flex-col gap-3xs p-0 pt-2xs">
                  <For each={giam()?.writableRoots ?? []}>
                    {(dir) => (
                      <li class="font-mono text-2xs break-all text-muted">{dir}</li>
                    )}
                  </For>
                </ul>
              )}
            />
          </Show>
          <Row
            icon="hand"
            label={t(S.settings.perms.blocksRow)}
            desc={t(S.settings.perms.blocksDesc)}
            more={t(S.settings.perms.blocksMore)}
          />
          <Row
            icon="plug"
            label={t(S.settings.perms.pluggedRow)}
            desc={t(S.settings.perms.pluggedDesc)}
            more={t(S.settings.perms.pluggedMore)}
            control={() => (
              <span
                class="text-2xs"
                classList={{
                  "text-faint": sandbox().tone === "faint",
                  "text-accent-ink": sandbox().tone === "ok",
                  "text-warn": sandbox().tone === "warn",
                  "text-danger": sandbox().tone === "danger",
                }}
              >
                {t(sandbox().text)}
              </span>
            )}
          />
        </RowGroup>

        <Show when={(cay()?.length ?? 0) > 0}>
          {/* The core's dump verbatim: a tidy summary would hide the very row causing trouble. */}
          <Disclosure
            label={t(S.settings.perms.treeLabel)}
            hint={tn(
              cay()?.length ?? 0,
              S.settings.perms.treeRowOne,
              S.settings.perms.treeRowMany,
            )}
          >
            <ul class="m-0 flex list-none flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-sm">
              <For each={cay()}>
                {(row) => (
                  <li class="flex flex-wrap items-baseline gap-2xs font-mono text-2xs">
                    <span class="text-ink">{row.id}</span>
                    <span class="text-muted">{row.plugin}</span>
                    <Show when={row.disabled}>
                      <span class="text-warn">{t(S.settings.perms.treeOff)}</span>
                    </Show>
                    <span class="min-w-0 text-faint">{row.origin}</span>
                  </li>
                )}
              </For>
            </ul>
          </Disclosure>
        </Show>
      </section>
    </div>
  );
}
