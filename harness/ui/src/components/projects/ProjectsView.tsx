import { Key } from "@solid-primitives/keyed";
import { createMemo, createSignal, For, Show } from "solid-js";
import { originHost } from "../../lib/projects";
import type { Project, ProjectKind } from "../../lib/protocol";
import { relativeTime } from "../../lib/sessions";
import Icon, { type IconName } from "../Icon";
import { Chip, IconButton } from "../primitives";
import CloneDialog from "./CloneDialog";
import ConfirmDialog from "./ConfirmDialog";
import { Button } from "./DialogShell";
import NewProjectDialog from "./NewProjectDialog";

type Filter = "all" | ProjectKind;

/**
 * Màn hình dự án: một trang đầy đủ, không phải một menu thả xuống.
 *
 * Menu cũ đủ dùng khi có ba dự án và không đủ khi có ba mươi — ở đó không có chỗ cho bộ
 * lọc, cho ô tìm, cũng không có chỗ để nói ba lối tạo dự án khác nhau ở điểm nào. Menu
 * vẫn còn giá trị của nó (đổi nhanh khi đang làm việc); trang này là chỗ *quyết định*.
 *
 * Hai câu chữ trên trang được viết rất cẩn thận và không nên rút gọn:
 *
 *   - "Bỏ khỏi danh sách" chứ không phải "Xoá". Lệnh phía dưới chỉ gỡ một dòng khỏi danh
 *     sách gần đây; thư mục trên đĩa không bị đụng. Người đọc "xoá dự án" hiểu là mất
 *     việc, và không có cách nào lấy lại niềm tin đó sau khi họ đã không dám bấm.
 *   - Dự án đang mở nói trước rằng nó không bỏ được. Lõi từ chối việc đó, nên để người
 *     dùng bấm rồi nhận một thông báo lỗi là bắt họ học một luật mà giao diện đã biết.
 */
export default function ProjectsView(props: {
  projects: Project[];
  /** Lõi đang tháo và cắm lại nhánh plugin: cả trang khoá cho tới khi xong. */
  switching?: boolean;
  /** Lỗi từ lần mở hoặc lần bỏ gần nhất, do chỗ gọi giữ. */
  error?: string | null;
  onOpen: (project: Project) => void;
  onForget: (project: Project) => void;
  /** Lõi đã tạo/clone xong; chỗ gọi nạp lại danh sách và chuyển sang dự án này. */
  onCreated: (project: Project) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [filter, setFilter] = createSignal<Filter>("all");
  const [newKind, setNewKind] = createSignal<ProjectKind | null>(null);
  const [cloning, setCloning] = createSignal(false);
  const [forgetting, setForgetting] = createSignal<Project | null>(null);

  // Mới nhất trước, giống menu dự án. Dự án đang mở không bị ghim lên đầu: nó đã có dấu
  // riêng rồi, và ghim thêm làm thứ tự nhảy mỗi lần đổi dự án.
  const visible = createMemo(() => {
    const needle = query().trim().toLowerCase();
    const kind = filter();
    return props.projects
      .filter((project) => kind === "all" || project.kind === kind)
      .filter(
        (project) =>
          needle === "" ||
          project.name.toLowerCase().includes(needle) ||
          project.path.toLowerCase().includes(needle),
      )
      .sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  });

  const counts = createMemo(() => ({
    all: props.projects.length,
    code: props.projects.filter((p) => p.kind === "code").length,
    docs: props.projects.filter((p) => p.kind === "docs").length,
  }));

  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-[880px] flex-col gap-2xl">
        <section class="flex flex-col gap-md">
          <div class="flex flex-col gap-3xs">
            <h2 class="m-0 text-md font-semibold text-ink">Dự án</h2>
            <p class="m-0 text-xs text-muted">
              Mỗi dự án là một thư mục trên máy. Trợ lý chỉ nhìn thấy thư mục của dự án
              đang mở.
            </p>
          </div>

          <div class="grid gap-sm sm:grid-cols-3">
            <For each={ENTRANCES}>
              {(entrance) => (
                <button
                  type="button"
                  disabled={props.switching}
                  onClick={() => {
                    if (entrance.id === "clone") setCloning(true);
                    else setNewKind(entrance.id);
                  }}
                  class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) text-left transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:border-accent enabled:hover:bg-accent-soft"
                >
                  <span class="flex items-center gap-2xs text-sm font-medium text-ink">
                    <Icon name={entrance.icon} size={15} />
                    {entrance.label}
                  </span>
                  <span class="text-2xs text-muted">{entrance.hint}</span>
                </button>
              )}
            </For>
          </div>
        </section>

        <Show when={props.error}>
          {(message) => (
            <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <section class="flex flex-col gap-md">
          <div class="flex flex-wrap items-center gap-sm">
            <label class="flex min-w-[220px] flex-1 items-center gap-2xs rounded-btn border border-line bg-surface px-sm focus-within:border-accent">
              <span class="shrink-0 text-faint">
                <Icon name="search" size={14} />
              </span>
              <input
                type="search"
                value={query()}
                spellcheck={false}
                placeholder="Tìm theo tên hoặc đường dẫn"
                aria-label="Tìm dự án theo tên hoặc đường dẫn"
                onInput={(event) => setQuery(event.currentTarget.value)}
                class="h-(--control-h) min-w-0 flex-1 bg-transparent text-xs text-text outline-none placeholder:text-faint"
              />
            </label>

            <div role="radiogroup" aria-label="Lọc theo loại dự án" class="flex gap-2xs">
              <For each={FILTERS}>
                {(option) => (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={filter() === option.id}
                    onClick={() => setFilter(option.id)}
                    class="flex items-center gap-2xs rounded-pill border px-md py-2xs text-xs transition-colors duration-[var(--dur-fast)]"
                    classList={{
                      "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                        filter() !== option.id,
                      "border-accent bg-accent-soft text-accent-ink": filter() === option.id,
                    }}
                  >
                    {option.label}
                    <span class="tabular-nums">{counts()[option.id]}</span>
                  </button>
                )}
              </For>
            </div>
          </div>

          <Show
            when={props.projects.length > 0}
            fallback={
              <div class="flex flex-col items-center gap-md rounded-card border border-dashed border-line px-(--card-pad-x) py-4xl text-center">
                <span class="grid size-12 place-items-center rounded-panel bg-accent-soft text-accent-ink">
                  <Icon name="folder-open" size={24} />
                </span>
                <div class="flex flex-col gap-2xs">
                  <p class="m-0 text-sm font-medium text-ink">Chưa có dự án nào</p>
                  <p class="m-0 max-w-[44ch] text-xs text-muted">
                    Mở một thư mục mã nguồn để trợ lý đọc và sửa được tệp, hoặc tạo một
                    thư viện tài liệu để hỏi đáp trên tài liệu của bạn.
                  </p>
                </div>
                <div class="flex flex-wrap justify-center gap-sm">
                  <Button variant="outline" icon="folder-open" onClick={() => setNewKind("code")}>
                    Mở thư mục mã nguồn
                  </Button>
                  <Button variant="outline" icon="library" onClick={() => setNewKind("docs")}>
                    Tạo thư viện tài liệu
                  </Button>
                </div>
              </div>
            }
          >
            <Show
              when={visible().length > 0}
              fallback={
                <p class="m-0 rounded-card border border-dashed border-line px-(--card-pad-x) py-2xl text-center text-xs text-muted">
                  Không có dự án nào khớp. Thử bỏ bộ lọc hoặc xoá bớt chữ trong ô tìm.
                </p>
              }
            >
              <ul class="m-0 flex list-none flex-col gap-sm p-0">
                {/* Keyed theo id: danh sách sắp xếp lại theo `lastOpenedAt` mỗi lần mở
                    một dự án, và keyed theo vị trí thì mọi hàng bị dựng lại — tiêu điểm
                    bàn phím rơi về `body` ngay giữa lúc người dùng đang đi bằng Tab. */}
                <Key each={visible()} by={(project) => project.id}>
                  {(keyed) => (
                    <Row
                      project={keyed()}
                      disabled={props.switching === true}
                      onOpen={() => props.onOpen(keyed())}
                      onForget={() => setForgetting(keyed())}
                    />
                  )}
                </Key>
              </ul>
            </Show>
          </Show>
        </section>
      </div>

      <Show when={newKind()}>
        {(kind) => (
          <NewProjectDialog
            kind={kind()}
            onClose={() => setNewKind(null)}
            onCreated={(project) => {
              setNewKind(null);
              props.onCreated(project);
            }}
          />
        )}
      </Show>

      <Show when={cloning()}>
        <CloneDialog
          onClose={() => setCloning(false)}
          onCreated={(project) => {
            setCloning(false);
            props.onCreated(project);
          }}
        />
      </Show>

      <Show when={forgetting()}>
        {(project) => (
          <ConfirmDialog
            icon="trash"
            title={`Bỏ "${project().name}" khỏi danh sách?`}
            body="Chỉ danh sách dự án gần đây bị đổi. Thư mục và toàn bộ tệp bên trong vẫn nguyên trên đĩa — mở lại thư mục này bất cứ lúc nào là dự án trở lại."
            detail={project().path}
            confirmLabel="Bỏ khỏi danh sách"
            onClose={() => setForgetting(null)}
            onConfirm={() => {
              const target = project();
              setForgetting(null);
              props.onForget(target);
            }}
          />
        )}
      </Show>
    </div>
  );
}

/** Một dự án. Hàng chứ không phải thẻ: đường dẫn dài cần cả bề ngang mới đọc được. */
function Row(props: {
  project: Project;
  disabled: boolean;
  onOpen: () => void;
  onForget: () => void;
}) {
  const current = () => props.project.isCurrent;
  return (
    <li
      aria-current={current() ? "true" : undefined}
      class="flex items-center gap-md rounded-card border bg-surface px-(--card-pad-x) py-(--card-pad-y) transition-colors duration-[var(--dur-fast)]"
      classList={{
        "border-line": !current(),
        "border-accent bg-accent-soft": current(),
      }}
    >
      <span
        class="grid size-8 shrink-0 place-items-center rounded-panel"
        classList={{
          "bg-accent text-on-accent": current(),
          "bg-[var(--overlay-faint)] text-muted": !current(),
        }}
      >
        <Icon name={props.project.kind === "docs" ? "library" : "code"} size={15} />
      </span>

      <div class="flex min-w-0 flex-1 flex-col gap-3xs">
        <div class="flex flex-wrap items-center gap-2xs">
          <span class="min-w-0 truncate text-sm font-medium text-ink">{props.project.name}</span>
          <Chip>{props.project.kind === "docs" ? "Tài liệu" : "Mã nguồn"}</Chip>
          {/* Huy hiệu nguồn gốc chỉ hiện host: một URL clone đầy đủ dài hơn cả tên dự án,
              và phần phân biệt được hai bản sao cùng tên là *máy chủ* chứ không phải
              đường dẫn. URL đầy đủ vẫn nằm ở `title` cho ai cần. */}
          <Show when={props.project.origin}>
            {(origin) => (
              <span title={origin()}>
                <Chip tone="accent">
                  <Icon name="git-branch" size={11} />
                  {originHost(origin())}
                </Chip>
              </span>
            )}
          </Show>
          <Show when={current()}>
            <Chip tone="accent">Đang mở</Chip>
          </Show>
        </div>
        {/* Đường dẫn cắt ở *đầu*: hai dự án cùng tên chỉ khác nhau ở phần đuôi. */}
        <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={props.project.path}>
          <bdi>{props.project.path}</bdi>
        </span>
      </div>

      <span class="hidden shrink-0 text-2xs whitespace-nowrap text-faint tabular-nums sm:inline">
        {relativeTime(props.project.lastOpenedAt)}
      </span>

      <div class="flex shrink-0 items-center gap-2xs">
        <Button
          variant="outline"
          disabled={props.disabled || current()}
          onClick={props.onOpen}
          label={current() ? `"${props.project.name}" đang mở` : `Mở "${props.project.name}"`}
        >
          {current() ? "Đang mở" : "Mở"}
        </Button>
        <IconButton
          icon="trash"
          size="sm"
          danger
          disabled={props.disabled || current()}
          onClick={props.onForget}
          label={
            current()
              ? `Không bỏ được "${props.project.name}" khỏi danh sách vì nó đang mở`
              : `Bỏ "${props.project.name}" khỏi danh sách. Thư mục trên đĩa không bị xoá.`
          }
          tip="left"
        />
      </div>
    </li>
  );
}

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "Tất cả" },
  { id: "code", label: "Mã nguồn" },
  { id: "docs", label: "Tài liệu" },
];

/**
 * Ba lối tạo dự án, hiện thành ba nút chứ không gộp thành một menu.
 *
 * Gộp lại thì lối clone và lối tạo thư viện nằm sau một cú bấm nữa, và một tính năng nằm
 * sau một cú bấm không ai biết là có thì bằng không tồn tại.
 */
const ENTRANCES: { id: ProjectKind | "clone"; label: string; icon: IconName; hint: string }[] = [
  {
    id: "code",
    label: "Mở thư mục mã nguồn",
    icon: "folder-open",
    hint: "Một thư mục đã có trên máy. Trợ lý đọc, sửa tệp và chạy được lệnh.",
  },
  {
    id: "clone",
    label: "Clone từ Git",
    icon: "git-branch",
    hint: "Tải repo về máy rồi mở nó làm dự án mã nguồn.",
  },
  {
    id: "docs",
    label: "Tạo thư viện tài liệu",
    icon: "library",
    hint: "Nạp PDF, Word, Markdown… để hỏi đáp. Chỉ tìm và đọc, không sửa gì.",
  },
];
