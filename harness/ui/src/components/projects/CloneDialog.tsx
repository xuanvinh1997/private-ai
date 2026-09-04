import { createSignal, Show } from "solid-js";
import { isDemo } from "../../lib/demo";
import { demoCloneFrames, demoCreatedProject } from "../../lib/fixtures/projects";
import { S, t, tn } from "../../lib/i18n";
import {
  cancelClone,
  cloneProject,
  pickDirectory,
  repoNameFromUrl,
} from "../../lib/projects";
import type { CloneProgress, Project } from "../../lib/protocol";
import { Disclosure } from "../primitives";
import { InfoDot } from "../settings/FormKit";
import DialogShell, { Button } from "./DialogShell";

/** Clone a repo and open it as a code project. Unmeasurable phases show a phase name rather than a stuck 0% bar, git's raw lines stay in a collapsible block because only git can say why it failed, and a clone can be cancelled mid-flight. */
export default function CloneDialog(props: {
  onCreated: (project: Project) => void;
  onClose: () => void;
}) {
  const [url, setUrl] = createSignal("");
  const [parent, setParent] = createSignal("");
  const [name, setName] = createSignal("");
  // Once typed, the name is never overwritten by the URL, or an edit silently disappears.
  const [nameTouched, setNameTouched] = createSignal(false);
  const [shallow, setShallow] = createSignal(true);

  const [running, setRunning] = createSignal(false);
  const [cancelled, setCancelled] = createSignal(false);
  const [phase, setPhase] = createSignal("");
  const [percent, setPercent] = createSignal<number | null>(null);
  const [lines, setLines] = createSignal<string[]>([]);
  const [error, setError] = createSignal<string | null>(null);

  const folder = () => (nameTouched() ? name() : repoNameFromUrl(url()));
  const target = () => {
    const base = parent().replace(/[/\\]+$/, "");
    const leaf = folder().trim();
    return base === "" || leaf === "" ? "" : `${base}/${leaf}`;
  };
  const ready = () => url().trim() !== "" && parent().trim() !== "" && folder().trim() !== "";

  /** `percent` is `null` in phases git cannot count; 0 would still be a real number. */
  const measured = () => percent() !== null;

  const note = (frame: CloneProgress) => {
    setPhase(frame.phase);
    setPercent(frame.percent);
    const line = frame.line;
    if (line !== null) {
      // Keep the last two hundred lines: a big clone emits thousands, and the reason is at the end.
      setLines((all) => [...all, line].slice(-200));
    }
  };

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickDirectory(t(S.projects.pickParent));
      if (picked !== null) setParent(picked);
    } catch (err) {
      setError(t(S.projects.pickError, { err: String(err) }));
    }
  };

  async function runDemo(): Promise<Project> {
    const frames = demoCloneFrames(url().trim(), target());
    for (const frame of frames) {
      await new Promise<void>((resolve) => setTimeout(resolve, 420));
      if (cancelled()) throw new Error("đã huỷ");
      note(frame);
    }
    return demoCreatedProject(target(), "code");
  }

  const start = async () => {
    if (!ready() || running()) return;
    setRunning(true);
    setCancelled(false);
    setError(null);
    setLines([]);
    setPercent(null);
    setPhase(t(S.projects.clonePreparing));
    try {
      const project = isDemo()
        ? await runDemo()
        : await cloneProject(
            {
              url: url().trim(),
              parent: parent().trim(),
              name: folder().trim(),
              ...(shallow() ? { depth: 1 } : {}),
            },
            note,
          );
      props.onCreated(project);
    } catch (err) {
      // A user cancelling is not an error, and colouring it red blames them for it.
      setError(cancelled() ? null : String(err));
      if (cancelled()) setPhase(t(S.projects.cloneCancelled));
    } finally {
      setRunning(false);
    }
  };

  /** Cancel while running, close otherwise; Esc and the cancel button share one door. */
  const dismiss = () => {
    if (!running()) {
      props.onClose();
      return;
    }
    setCancelled(true);
    setPhase(t(S.projects.cloneCancelling));
    void cancelClone();
  };

  return (
    <DialogShell
      icon="git-branch"
      title={t(S.projects.cloneTitle)}
      desc={t(S.projects.cloneDesc)}
      busy={running()}
      width="lg"
      onClose={dismiss}
      footer={() => (
        <>
          <Button onClick={dismiss}>
            {running() ? t(S.projects.cancelClone) : t(S.common.close)}
          </Button>
          <Button
            variant="primary"
            onClick={() => void start()}
            disabled={running() || !ready()}
          >
            {running() ? t(S.projects.cloning) : t(S.projects.clone)}
          </Button>
        </>
      )}
    >
      <label class="flex flex-col gap-2xs">
        <span class="text-2xs text-faint">{t(S.projects.repoUrl)}</span>
        <input
          type="text"
          value={url()}
          spellcheck={false}
          autocapitalize="off"
          autocomplete="off"
          placeholder={t(S.projects.repoUrlPlaceholder)}
          disabled={running()}
          onInput={(event) => {
            setUrl(event.currentTarget.value);
            setError(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void start();
            }
          }}
          class="h-(--cta-h) rounded-btn border border-line-strong bg-bg px-sm font-mono text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
        />
      </label>

      <div class="grid gap-sm sm:grid-cols-[1fr_auto]">
        <label class="flex min-w-0 flex-col gap-2xs">
          <span class="text-2xs text-faint">{t(S.projects.parentFolder)}</span>
          <div class="flex gap-sm">
            <input
              type="text"
              value={parent()}
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              placeholder={t(S.projects.parentPlaceholder)}
              disabled={running()}
              onInput={(event) => setParent(event.currentTarget.value)}
              class="h-(--cta-h) min-w-0 flex-1 rounded-btn border border-line-strong bg-bg px-sm font-mono text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
            />
            <Button icon="folder-open" variant="outline" disabled={running()} onClick={() => void choose()}>
              {t(S.projects.choose)}
            </Button>
          </div>
        </label>

        <label class="flex flex-col gap-2xs">
          <span class="text-2xs text-faint">{t(S.projects.folderName)}</span>
          <input
            type="text"
            value={folder()}
            spellcheck={false}
            autocapitalize="off"
            autocomplete="off"
            placeholder="repo"
            disabled={running()}
            onInput={(event) => {
              setNameTouched(true);
              setName(event.currentTarget.value);
            }}
            class="h-(--cta-h) w-full rounded-btn border border-line-strong bg-bg px-sm font-mono text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50 sm:w-44"
          />
        </label>
      </div>

      <Show when={target() !== ""}>
        <p class="m-0 min-w-0 truncate rounded-panel bg-surface-soft px-sm py-2xs font-mono text-2xs text-muted" dir="rtl" title={target()}>
          <bdi>{target()}</bdi>
        </p>
      </Show>

      <label class="flex items-start gap-sm rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
        <input
          type="checkbox"
          checked={shallow()}
          disabled={running()}
          onChange={(event) => setShallow(event.currentTarget.checked)}
          class="mt-3xs size-4 shrink-0 accent-[var(--accent)]"
        />
        <span class="flex flex-col gap-3xs">
          <span class="flex items-center gap-2xs text-xs text-text">
            {t(S.projects.shallow)}
            <InfoDot label={t(S.projects.shallowLabel)} text={t(S.projects.shallowMore)} />
          </span>
          <span class="text-2xs text-faint">{t(S.projects.shallowHint)}</span>
        </span>
      </label>

      <Show when={running() || phase() !== ""}>
        <div class="flex flex-col gap-2xs rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
          <div class="flex items-baseline justify-between gap-sm">
            <span class="text-xs text-text" role="status" aria-live="polite">
              {phase()}
            </span>
            <Show when={measured()}>
              <span class="text-2xs text-muted tabular-nums">{percent() ?? 0}%</span>
            </Show>
          </div>

          <Show
            when={measured()}
            fallback={
              // With no number, do not fake one: a moving band says working, a stuck 0% says broken.
              <div
                role="progressbar"
                aria-label={t(S.projects.cloneProgress, { phase: phase() })}
                class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
              >
                <div class="h-full w-1/3 rounded-pill bg-accent motion-safe:animate-pulse" />
              </div>
            }
          >
            <div
              role="progressbar"
              aria-valuenow={percent() ?? 0}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-label={t(S.projects.cloneProgress, { phase: phase() })}
              class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
            >
              <div
                class="h-full rounded-pill bg-accent transition-[width] duration-[var(--dur-base)]"
                style={{ width: `${percent() ?? 0}%` }}
              />
            </div>
          </Show>

          <Show when={lines().length > 0}>
            <Disclosure
              label={t(S.projects.details)}
              hint={tn(lines().length, S.projects.lineOne, S.projects.lineMany)}
            >
              <pre class="m-0 max-h-40 overflow-auto rounded-panel bg-bg p-sm font-mono text-2xs whitespace-pre text-muted">
                {lines().join("\n")}
              </pre>
            </Disclosure>
          </Show>
        </div>
      </Show>

      <Show when={error()}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>
    </DialogShell>
  );
}
