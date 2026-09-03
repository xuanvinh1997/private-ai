import { createSignal, Show } from "solid-js";
import { isDemo } from "../../lib/demo";
import { demoCloneFrames, demoCreatedProject } from "../../lib/fixtures/projects";
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

/**
 * Clone một repo về máy rồi mở nó làm dự án mã nguồn.
 *
 * Ba quyết định đáng nói, cả ba đều về lúc mọi thứ *không* chạy trơn:
 *
 * **Không có `percent` thì hiện tên pha, không hiện thanh 0%.** `git` không đếm được ở
 * pha đếm đối tượng và pha phân giải tên miền, mà đó lại là hai pha lâu nhất trên một
 * đường mạng tồi. Một thanh đứng im ở 0% trong mười giây trông giống hệt một tiến trình
 * đã treo, và người dùng sẽ đóng cửa sổ đi.
 *
 * **Dòng thô của git giữ nguyên trong một khối gập được.** Khi clone hỏng, câu duy nhất
 * nói được nguyên nhân là câu của chính git — "Permission denied (publickey)" dạy được
 * người dùng phải làm gì, còn "clone thất bại" thì không.
 *
 * **Huỷ được giữa chừng.** Một lần clone có thể kéo dài vài phút, và không có lối thoát
 * thì lối thoát duy nhất là giết ứng dụng.
 */
export default function CloneDialog(props: {
  onCreated: (project: Project) => void;
  onClose: () => void;
}) {
  const [url, setUrl] = createSignal("");
  const [parent, setParent] = createSignal("");
  const [name, setName] = createSignal("");
  // Tên do người dùng gõ thì không bị URL ghi đè nữa — sửa xong mà bị nuốt là một cái
  // bẫy im lặng, người dùng chỉ phát hiện ra sau khi thư mục đã nằm sai chỗ.
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

  /** `percent` là `null` ở những pha git không đếm được — 0 vẫn là một con số thật. */
  const measured = () => percent() !== null;

  const note = (frame: CloneProgress) => {
    setPhase(frame.phase);
    setPercent(frame.percent);
    const line = frame.line;
    if (line !== null) {
      // Giữ hai trăm dòng cuối. Một lần clone lớn phát ra hàng nghìn dòng, và phần nói
      // được nguyên nhân hỏng luôn nằm ở cuối.
      setLines((all) => [...all, line].slice(-200));
    }
  };

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickDirectory("Chọn thư mục cha");
      if (picked !== null) setParent(picked);
    } catch (err) {
      setError(`Không mở được hộp thoại chọn thư mục: ${err}`);
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
    setPhase("Đang chuẩn bị");
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
      // Người dùng tự huỷ thì đó không phải lỗi, và tô nó đỏ là đổ lỗi cho họ.
      setError(cancelled() ? null : String(err));
      if (cancelled()) setPhase("Đã huỷ");
    } finally {
      setRunning(false);
    }
  };

  /** Huỷ nếu đang chạy, đóng nếu không. Esc và nút "Huỷ" đi chung một cửa. */
  const dismiss = () => {
    if (!running()) {
      props.onClose();
      return;
    }
    setCancelled(true);
    setPhase("Đang huỷ…");
    void cancelClone();
  };

  return (
    <DialogShell
      icon="git-branch"
      title="Clone từ Git"
      desc="Tải một repo về máy rồi mở làm dự án."
      busy={running()}
      width="lg"
      onClose={dismiss}
      footer={() => (
        <>
          <Button onClick={dismiss}>{running() ? "Huỷ clone" : "Đóng"}</Button>
          <Button
            variant="primary"
            onClick={() => void start()}
            disabled={running() || !ready()}
          >
            {running() ? "Đang clone…" : "Clone"}
          </Button>
        </>
      )}
    >
      <label class="flex flex-col gap-2xs">
        <span class="text-2xs text-faint">URL repo</span>
        <input
          type="text"
          value={url()}
          spellcheck={false}
          autocapitalize="off"
          autocomplete="off"
          placeholder="https://github.com/ten/repo.git"
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
          class="h-(--cta-h) rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
        />
      </label>

      <div class="grid gap-sm sm:grid-cols-[1fr_auto]">
        <label class="flex min-w-0 flex-col gap-2xs">
          <span class="text-2xs text-faint">Thư mục cha</span>
          <div class="flex gap-sm">
            <input
              type="text"
              value={parent()}
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              placeholder="/Users/ban/Workspaces"
              disabled={running()}
              onInput={(event) => setParent(event.currentTarget.value)}
              class="h-(--cta-h) min-w-0 flex-1 rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
            />
            <Button icon="folder-open" variant="outline" disabled={running()} onClick={() => void choose()}>
              Chọn…
            </Button>
          </div>
        </label>

        <label class="flex flex-col gap-2xs">
          <span class="text-2xs text-faint">Tên thư mục</span>
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
            class="h-(--cta-h) w-full rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50 sm:w-44"
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
            Chỉ lấy lịch sử gần nhất
            <InfoDot
              label="Về việc chỉ lấy lịch sử gần nhất"
              text="Nhanh hơn nhiều và đủ để đọc mã. Đổi lại, không xem được lịch sử cũ và không đẩy ngược lên nhánh khác — cần thì clone lại đầy đủ."
            />
          </span>
          <span class="text-2xs text-faint">Nhanh hơn nhiều và đủ để đọc mã.</span>
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
              // Không có số thì không giả vờ có: một dải chạy qua chạy lại nói "đang
              // làm gì đó", còn một thanh đứng im ở 0% nói "đã hỏng".
              <div
                role="progressbar"
                aria-label={`Đang clone: ${phase()}`}
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
              aria-label={`Đang clone: ${phase()}`}
              class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
            >
              <div
                class="h-full rounded-pill bg-accent transition-[width] duration-[var(--dur-base)]"
                style={{ width: `${percent() ?? 0}%` }}
              />
            </div>
          </Show>

          <Show when={lines().length > 0}>
            <Disclosure label="Chi tiết" hint={`${lines().length} dòng`}>
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
