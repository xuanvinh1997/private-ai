import { createSignal, Show } from "solid-js";
import { S, t, tn } from "../../lib/i18n";
import { importProjectFiles, pickProjectFiles } from "../../lib/projects";
import type { Project } from "../../lib/protocol";
import DropZone from "../docs/DropZone";
import DialogShell, { Button } from "./DialogShell";

/** Import local files into a project. Copying happens in the Rust core so the webview never receives file
 * contents, and the dialog remains open afterwards to show an explicit result or accept another batch. */
export default function UploadFilesDialog(props: {
  project: Project;
  onClose: () => void;
  onImported: (paths: string[]) => void;
}) {
  const [busy, setBusy] = createSignal(false);
  const [uploaded, setUploaded] = createSignal(0);
  const [error, setError] = createSignal<string | null>(null);

  const close = () => {
    if (!busy()) props.onClose();
  };

  const upload = async (paths: string[]) => {
    if (paths.length === 0 || busy()) return;
    setBusy(true);
    setUploaded(0);
    setError(null);
    try {
      const imported = await importProjectFiles(paths);
      if (imported.length === 0) return;
      setUploaded(imported.length);
      props.onImported(imported);
    } catch (err) {
      setError(t(S.projects.uploadError, { err: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const pick = async () => {
    setError(null);
    try {
      await upload(await pickProjectFiles(t(S.projects.uploadChoose)));
    } catch (err) {
      setError(t(S.common.pickerFailed, { err: String(err) }));
    }
  };

  return (
    <DialogShell
      icon="upload"
      title={t(S.projects.uploadTitle)}
      desc={t(S.projects.uploadDesc, { name: props.project.name })}
      more={t(S.projects.uploadMore)}
      busy={busy()}
      onClose={close}
      footer={() => (
        <Button variant="outline" disabled={busy()} onClick={close}>
          {uploaded() > 0 ? t(S.common.done) : t(S.common.close)}
        </Button>
      )}
    >
      <DropZone
        busy={busy()}
        labels={{
          title: t(S.projects.uploadDropTitle),
          hint: t(S.projects.uploadDropHint),
          more: t(S.projects.uploadMore),
          pick: t(S.projects.uploadChoose),
        }}
        onPaths={(paths) => void upload(paths)}
        onPick={() => void pick()}
      />

      <Show when={busy()}>
        <p class="m-0 text-xs text-muted" role="status" aria-live="polite">
          {t(S.projects.uploading)}
        </p>
      </Show>

      <Show when={uploaded() > 0}>
        <p
          class="m-0 rounded-panel bg-success-soft px-sm py-2xs text-xs text-success"
          role="status"
          aria-live="polite"
        >
          {tn(uploaded(), S.projects.uploadDoneOne, S.projects.uploadDoneMany)}
        </p>
      </Show>

      <Show when={error()}>
        {(message) => (
          <p
            class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger"
            role="alert"
          >
            {message()}
          </p>
        )}
      </Show>
    </DialogShell>
  );
}
