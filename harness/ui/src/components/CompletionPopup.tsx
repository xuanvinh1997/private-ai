import { createEffect, For, Show } from "solid-js";
import Icon from "./Icon";

/** Một dòng trong danh sách gợi ý. */
export interface Suggestion {
  /** Chuỗi sẽ được chèn vào ô nhập. */
  value: string;
  /** Chữ hiện đậm ở đầu dòng. Mặc định bằng `value`. */
  label?: string;
  /** Câu phụ bên phải. */
  hint?: string;
  /** Không chọn được, kèm lý do nói ra ở `hint`. */
  disabled?: boolean;
}

/**
 * Danh sách gợi ý nổi trên ô soạn tin, cho cả `@` lẫn `/`.
 *
 * # Vì sao tiêu điểm không rời ô nhập
 *
 * Người dùng đang **gõ**. Đưa tiêu điểm sang danh sách thì mỗi phím tiếp theo đi nhầm chỗ,
 * và họ phải bấm quay lại để gõ tiếp — một cái bẫy cho đúng thao tác mà bộ hoàn thành sinh
 * ra để làm nhanh hơn. Nên danh sách chỉ vẽ một con trỏ, còn phím vẫn thuộc về ô nhập.
 *
 * Cái giá của lựa chọn ấy là ARIA phải làm đúng: ô nhập bên kia khai `combobox` và trỏ vào
 * hàng đang sáng bằng `aria-activedescendant`. Thiếu nó thì người dùng trình đọc màn hình
 * bấm mũi tên và **không nghe thấy gì**, vì thứ duy nhất đổi là một màu nền. Đây là đúng
 * lỗi mà bảng lệnh phiên đã mắc, nên nó được viết ra ở cả hai chỗ.
 */
export default function CompletionPopup(props: {
  items: Suggestion[];
  cursor: number;
  /** Id của phần tử danh sách, để ô nhập trỏ `aria-controls` vào. */
  id: string;
  /** Dựng id cho từng hàng, để ô nhập trỏ `aria-activedescendant` vào. */
  optionId: (index: number) => string;
  onPick: (item: Suggestion) => void;
  onHover: (index: number) => void;
  /** Câu hiện khi không có gợi ý nào. Không truyền thì danh sách rỗng không vẽ gì. */
  empty?: string;
}) {
  let list: HTMLUListElement | undefined;

  // Con trỏ đi bằng bàn phím phải kéo theo khung nhìn, cùng lý do như ở bảng lệnh phiên.
  createEffect(() => {
    const index = props.cursor;
    if (props.items.length === 0) return;
    list?.children[index]?.scrollIntoView({ block: "nearest" });
  });

  return (
    <Show when={props.items.length > 0 || props.empty !== undefined}>
      <div
        class="absolute bottom-full left-0 right-0 z-20 mb-2xs overflow-hidden rounded-panel border border-line bg-surface shadow-pop"
        // Bấm vào danh sách không được cướp tiêu điểm khỏi ô nhập: mất tiêu điểm là ô soạn
        // tin thu lại và danh sách đóng trước cả khi cú bấm kịp thành một lựa chọn.
        onMouseDown={(event) => event.preventDefault()}
      >
        <Show
          when={props.items.length > 0}
          fallback={<p class="m-0 px-md py-sm text-sm text-faint">{props.empty}</p>}
        >
          <ul
            ref={list}
            id={props.id}
            role="listbox"
            class="m-0 max-h-[240px] list-none overflow-y-auto p-2xs"
          >
            <For each={props.items}>
              {(item, index) => (
                <li role="presentation">
                  <button
                    type="button"
                    id={props.optionId(index())}
                    role="option"
                    aria-selected={index() === props.cursor}
                    aria-disabled={item.disabled === true}
                    onClick={() => {
                      if (item.disabled !== true) props.onPick(item);
                    }}
                    onMouseEnter={() => props.onHover(index())}
                    class="flex w-full items-center gap-xs rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink aria-[disabled=true]:opacity-50"
                  >
                    <span class="min-w-0 flex-1 truncate font-mono text-sm">
                      {item.label ?? item.value}
                    </span>
                    <Show when={item.hint}>
                      <span class="shrink-0 text-2xs text-faint">{item.hint}</span>
                    </Show>
                    <Show when={item.disabled !== true}>
                      <Icon name="enter" size={12} />
                    </Show>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </Show>
  );
}
