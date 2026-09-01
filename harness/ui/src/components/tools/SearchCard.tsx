import { For, Show } from "solid-js";
import type { ToolCall } from "../../lib/protocol";
import { useTranscriptActions } from "../../lib/transcriptActions";
import { Disclosure, FilePath } from "../primitives";
import { ToolShell } from "./ToolCard";

/** Bao nhiêu khớp hiện ngay không cần mở. Đủ để nhận ra kết quả có đúng hướng không. */
const PEEK = 3;

function pattern(call: ToolCall): string {
  const bag = call.args as Record<string, unknown> | null;
  if (bag === null || typeof bag !== "object") return "";
  const raw = bag.pattern ?? bag.query ?? bag.glob;
  return typeof raw === "string" ? raw : "";
}

/**
 * Thẻ `grep`: nhóm theo tệp, mỗi nhóm vài dòng khớp.
 *
 * `truncated` được nói thẳng ra thay vì im lặng cắt: "không tìm thấy gì khác" và "đã
 * ngừng đếm" là hai kết luận rất khác nhau cho người đang đọc.
 */
export function GrepCard(props: { call: ToolCall }) {
  const search = () => props.call.meta?.search;
  const groups = () => search()?.groups ?? [];

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">{pattern(props.call)}</code>
          <Show when={search()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {meta().total} khớp · {groups().length} tệp{meta().truncated ? " · đã cắt bớt" : ""}
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={groups().length > 0}>
        <Disclosure label="Kết quả" hint={`${groups().length} tệp`} open>
          <ul class="flex flex-col gap-2xs">
            <For each={groups()}>
              {(group) => (
                <li class="rounded-panel bg-surface px-sm py-2xs">
                  <FilePath path={group.path} line={group.matches[0]?.line} />
                  <div class="mt-3xs overflow-x-auto">
                    <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
                      <For each={group.matches.slice(0, PEEK)}>
                        {(match) => <MatchRow path={group.path} line={match.line} text={match.text} />}
                      </For>
                    </div>
                  </div>
                  <Show when={group.matches.length > PEEK}>
                    <p class="mt-3xs text-2xs text-faint">
                      còn {group.matches.length - PEEK} khớp nữa trong tệp này
                    </p>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Disclosure>
      </Show>
    </ToolShell>
  );
}

/**
 * Một dòng khớp.
 *
 * Cả dòng bấm được chứ không chỉ đường dẫn ở trên: người ta tìm bằng grep để đi tới một
 * *chỗ*, và cái chỗ đó là dòng này chứ không phải đầu tệp.
 */
function MatchRow(props: { path: string; line: number; text: string }) {
  const actions = useTranscriptActions();
  const open = () => actions.openFile;
  return (
    <Show
      when={open()}
      fallback={
        <div class="flex items-start gap-sm">
          <span class="w-10 shrink-0 text-right text-faint tabular-nums select-none">{props.line}</span>
          <span class="whitespace-pre text-text">{props.text}</span>
        </div>
      }
    >
      {(go) => (
        <button
          type="button"
          onClick={() => go()(props.path, props.line)}
          title={`Mở ${props.path} ở dòng ${props.line}`}
          class="flex w-full items-start gap-sm rounded-btn text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
        >
          <span class="w-10 shrink-0 text-right text-faint tabular-nums select-none">{props.line}</span>
          <span class="whitespace-pre text-text">{props.text}</span>
        </button>
      )}
    </Show>
  );
}

/** Thẻ `glob`: chỉ có danh sách đường dẫn, không có dòng nội dung nào. */
export function GlobCard(props: { call: ToolCall }) {
  const search = () => props.call.meta?.search;
  const paths = () => search()?.paths ?? [];

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">{pattern(props.call)}</code>
          <Show when={search()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {meta().total} tệp{meta().truncated ? " · đã cắt bớt" : ""}
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={paths().length > 0}>
        <Disclosure label="Đường dẫn" hint={`${paths().length}`}>
          <ul class="max-h-56 overflow-auto rounded-panel bg-surface px-sm py-2xs">
            <For each={paths()}>
              {(path) => (
                <li class="flex">
                  <FilePath path={path} />
                </li>
              )}
            </For>
          </ul>
        </Disclosure>
      </Show>
    </ToolShell>
  );
}
