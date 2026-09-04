import { createSignal, For, Show } from "solid-js";
import { useDragDrop } from "../../hooks/useDragDrop";
import { isDemo } from "../../lib/demo";
import { demoCreatedProject } from "../../lib/fixtures/projects";
import { S, t, type Msg } from "../../lib/i18n";
import { createProject, pickDirectory } from "../../lib/projects";
import type { Project, ProjectKind } from "../../lib/protocol";
import Icon, { type IconName } from "../Icon";
import { InfoDot } from "../settings/FormKit";
import DialogShell, { Button } from "./DialogShell";

/** Pick the project kind, then the folder. Kind comes first because it is the hard choice to undo, and the path has three ways in: the OS dialog, drag and drop, and typing. */
export default function NewProjectDialog(props: {
  /** Preselected kind: the buttons on the project screen already said what the user wants. */
  kind?: ProjectKind;
  onCreated: (project: Project) => void;
  onClose: () => void;
}) {
  const [kind, setKind] = createSignal<ProjectKind>(props.kind ?? "code");
  const [path, setPath] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // A drop while this dialog is open fills the field rather than opening: the kind is still unanswered.
  useDragDrop((paths) => {
    const first = paths[0];
    if (first !== undefined && !busy()) {
      setPath(first);
      setError(null);
    }
  });

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickDirectory(
        kind() === "code" ? t(S.projects.pickCode) : t(S.projects.pickDocs),
      );
      if (picked !== null) setPath(picked);
    } catch (err) {
      setError(t(S.projects.pickError, { err: String(err) }));
    }
  };

  const submit = async () => {
    const trimmed = path().trim();
    if (trimmed === "" || busy()) return;
    setBusy(true);
    setError(null);
    try {
      const project = isDemo()
        ? await new Promise<Project>((resolve) =>
            setTimeout(() => resolve(demoCreatedProject(trimmed, kind())), 600),
          )
        : await createProject(trimmed, kind());
      props.onCreated(project);
    } catch (err) {
      // Verbatim from the core: only it knows whether the folder exists, is readable, or is taken.
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      icon="plus"
      title={t(S.projects.newTitle)}
      desc={t(S.projects.newDesc)}
      more={t(S.projects.newMore)}
      busy={busy()}
      width="lg"
      onClose={() => {
        if (!busy()) props.onClose();
      }}
      footer={() => (
        <>
          <Show when={busy()}>
            <span class="mr-auto text-2xs text-muted" role="status" aria-live="polite">
              {t(S.projects.opening)}
            </span>
          </Show>
          <Button onClick={props.onClose} disabled={busy()}>
            {t(S.common.cancel)}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy() || path().trim() === ""}
          >
            {t(S.projects.create)}
          </Button>
        </>
      )}
    >
      <div
        role="radiogroup"
        aria-label={t(S.projects.kindLabel)}
        class="grid gap-sm sm:grid-cols-2"
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
          event.preventDefault();
          const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
          const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
          const delta = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
          const next = (current + delta + buttons.length) % buttons.length;
          const option = KINDS[next];
          buttons[next]?.focus();
          if (option !== undefined) setKind(option.id);
        }}
      >
        <For each={KINDS}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={kind() === option.id}
              tabIndex={kind() === option.id ? 0 : -1}
              disabled={busy()}
              onClick={() => setKind(option.id)}
              class="flex flex-col gap-2xs rounded-card border p-(--card-pad-x) text-left transition-colors duration-[var(--dur-fast)] disabled:opacity-50"
              classList={{
                "border-line bg-surface-soft hover:border-line-strong": kind() !== option.id,
                "border-accent bg-accent-soft": kind() === option.id,
              }}
            >
              <span class="flex items-center gap-2xs text-sm font-medium text-ink">
                <Icon name={option.icon} size={15} />
                {t(option.label)}
              </span>
              <span class="text-2xs text-muted">{t(option.can)}</span>
              <span class="text-2xs text-faint">{t(option.cannot)}</span>
            </button>
          )}
        </For>
      </div>

      {/* Outside both cards, because it is about the cost of choosing wrong, not about either kind. */}
      <p class="m-0 flex items-center gap-2xs text-2xs text-faint">
        {t(S.projects.kindWarn)}
        <InfoDot text={t(S.projects.kindMore)} />
      </p>

      <div class="flex flex-col gap-2xs">
        <label class="flex flex-col gap-2xs">
          <span class="text-2xs text-faint">{t(S.projects.folder)}</span>
          <div class="flex gap-sm">
            <input
              type="text"
              value={path()}
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              placeholder={t(S.projects.folderPlaceholder)}
              disabled={busy()}
              onInput={(event) => setPath(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
              class="h-(--cta-h) min-w-0 flex-1 rounded-btn border border-line-strong bg-bg px-sm font-mono text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
            />
            <Button icon="folder-open" variant="outline" disabled={busy()} onClick={() => void choose()}>
              {t(S.projects.choose)}
            </Button>
          </div>
        </label>
        <p class="m-0 text-2xs text-faint">{t(S.projects.dropHint)}</p>
      </div>

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

/** The two kinds, described by what the assistant can do, since the names say nothing about that. */
const KINDS: { id: ProjectKind; label: Msg; icon: IconName; can: Msg; cannot: Msg }[] = [
  {
    id: "code",
    label: S.projects.kindCode,
    icon: "code",
    can: S.projects.kindCodeCan,
    cannot: S.projects.kindCodeCannot,
  },
  {
    id: "docs",
    label: S.projects.kindDocs,
    icon: "library",
    can: S.projects.kindDocsCan,
    cannot: S.projects.kindDocsCannot,
  },
];
