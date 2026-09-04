import { createResource, For, Show } from "solid-js";
import { inTauri } from "../../lib/agent";
import { S, t } from "../../lib/i18n";
import { CopyButton } from "../primitives";
import { Banner, Row, RowGroup, SectionHead } from "./FormKit";
import { daVa, describeHarness, docCayPlugin, listHooks } from "./harness";

/** The Hooks page is read-only on purpose, since the core has no command to add or edit a hook. It says what hooks are, states the two counter-intuitive facts up front (fail-open, and unsandboxed), and points at the file to edit. */

/** The config file path, hard-coded and possibly wrong: the core reads `PAI_DATA_DIR` first. */
const TEP_VA = "~/.private-ai/patch.yaml";

/** A sample to paste into the patch file; `replace`, not `insert`, since the `hooks` row already exists. */
const MAU = `patches:
  - op: replace
    id: hooks
    config:
      hooks:
        - command: "jq -e '.arguments.command | test(\\"rm -rf\\") | not' >/dev/null && echo '{\\"decision\\":\\"allow\\"}' || echo '{\\"decision\\":\\"deny\\",\\"reason\\":\\"khong chay rm -rf\\"}'"
          tools: ["bash"]
          timeout_secs: 5`;

export default function HooksPage() {
  // The plugin tree answers one thing: has a user layer patched the `hooks` row at all.
  const [cay] = createResource(async () => docCayPlugin(await describeHarness()));
  // The real hook list, from the composed config row; empty is the default, not an error.
  const [hooks] = createResource(listHooks);

  /** One place deciding both the description and the right-column label, so the two cannot drift. */
  const trangThai = (): {
    nhan: string;
    moTa: string;
    them?: string;
    tone: "faint" | "ok" | "muted";
  } => {
    if (!inTauri())
      return {
        nhan: t(S.settings.hooks.stateDemo),
        moTa: t(S.settings.hooks.stateDemoDesc),
        tone: "faint",
      };
    if (cay.loading)
      return {
        nhan: t(S.settings.hooks.stateAsking),
        moTa: t(S.settings.hooks.stateAskingDesc),
        tone: "faint",
      };
    if (cay.error !== undefined)
      return {
        nhan: t(S.settings.hooks.stateError),
        moTa: t(S.settings.hooks.stateErrorDesc),
        tone: "faint",
      };
    const row = cay()?.find((item) => item.id === "hooks");
    if (row === undefined)
      return {
        nhan: t(S.settings.hooks.stateMissing),
        moTa: t(S.settings.hooks.stateMissingDesc),
        them: t(S.settings.hooks.stateMissingMore),
        tone: "muted",
      };
    if (!daVa(row))
      return {
        nhan: t(S.settings.hooks.stateEmpty),
        moTa: t(S.settings.hooks.stateEmptyDesc),
        them: t(S.settings.hooks.stateEmptyMore),
        tone: "muted",
      };
    return {
      nhan: t(S.settings.hooks.statePatched),
      moTa: t(S.settings.hooks.statePatchedDesc, { origin: row.origin }),
      them: t(S.settings.hooks.statePatchedMore, { origin: row.origin }),
      tone: "ok",
    };
  };

  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          icon="warn"
          title={t(S.settings.hooks.warnTitle)}
          desc={t(S.settings.hooks.warnDesc)}
        />

        <RowGroup>
          <Row
            icon="warn"
            label={t(S.settings.hooks.failOpen)}
            desc={t(S.settings.hooks.failOpenDesc)}
            more={t(S.settings.hooks.failOpenMore)}
            control={() => (
              <span class="text-2xs text-warn">{t(S.settings.hooks.failOpenTag)}</span>
            )}
          />
          <Row
            icon="shield"
            label={t(S.settings.hooks.unsandboxed)}
            desc={t(S.settings.hooks.unsandboxedDesc)}
            more={t(S.settings.hooks.unsandboxedMore)}
            control={() => (
              <span class="text-2xs text-warn">{t(S.settings.hooks.unsandboxedTag)}</span>
            )}
          />
          <Row
            icon="hand"
            label={t(S.settings.hooks.noRewrite)}
            desc={t(S.settings.hooks.noRewriteDesc)}
            more={t(S.settings.hooks.noRewriteMore)}
            control={() => (
              <span class="text-2xs text-faint">{t(S.settings.hooks.noRewriteTag)}</span>
            )}
          />
        </RowGroup>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          icon="list"
          title={t(S.settings.hooks.listTitle)}
          desc={t(S.settings.hooks.listDesc)}
          more={t(S.settings.hooks.listMore)}
        />

        <Show
          when={(hooks() ?? []).length > 0}
          fallback={
            <RowGroup>
              <Row
                icon="check"
                label={t(S.settings.hooks.listEmpty)}
                desc={t(S.settings.hooks.listEmptyDesc)}
                more={t(S.settings.hooks.listEmptyMore)}
              />
            </RowGroup>
          }
        >
          <RowGroup>
            <For each={hooks() ?? []}>
              {(hook) => (
                <Row
                  label={hook.command}
                  labelMono
                  desc={t(S.settings.hooks.itemLine, {
                    tools:
                      hook.tools.length === 0
                        ? t(S.settings.hooks.itemAllTools)
                        : t(S.settings.hooks.itemSomeTools, { list: hook.tools.join(", ") }),
                    secs: hook.timeoutSecs ?? 10,
                    origin: hook.origin,
                  })}
                />
              )}
            </For>
          </RowGroup>
        </Show>

        <RowGroup>
          <Row
            icon="plug"
            label={t(S.settings.hooks.rowLabel)}
            desc={trangThai().moTa}
            more={trangThai().them}
            control={() => (
              <span
                class="text-2xs"
                classList={{
                  "text-faint": trangThai().tone === "faint",
                  "text-muted": trangThai().tone === "muted",
                  "text-accent-ink": trangThai().tone === "ok",
                }}
              >
                {trangThai().nhan}
              </span>
            )}
          />
        </RowGroup>

        <Banner
          tone="info"
          icon="warn"
          title={t(S.settings.hooks.readOnlyTitle)}
          more={t(S.settings.hooks.readOnlyMore)}
        >
          {t(S.settings.hooks.readOnlyBody)}
        </Banner>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          icon="pencil"
          title={t(S.settings.hooks.manualTitle)}
          desc={t(S.settings.hooks.manualDesc)}
          more={t(S.settings.hooks.manualMore)}
        />

        <RowGroup>
          <Row
            icon="document"
            label={TEP_VA}
            labelMono
            desc={t(S.settings.hooks.fileDesc)}
            more={t(S.settings.hooks.fileMore)}
            control={() => (
              <CopyButton text={() => TEP_VA} label={t(S.settings.hooks.copyPath)} />
            )}
          />
          <Row
            icon="code"
            label={t(S.settings.hooks.fields)}
            desc={t(S.settings.hooks.fieldsDesc)}
            more={t(S.settings.hooks.fieldsMore)}
          />
        </RowGroup>

        <div class="flex flex-col gap-2xs">
          <div class="flex items-center justify-between gap-sm">
            <span class="text-2xs text-faint">{t(S.settings.hooks.sampleCaption)}</span>
            <CopyButton text={() => MAU} label={t(S.settings.hooks.copySample)} />
          </div>
          <pre class="m-0 overflow-x-auto rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-sm font-mono text-2xs text-text">
            {MAU}
          </pre>
        </div>
      </section>
    </div>
  );
}
