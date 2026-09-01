import { For, Show } from "solid-js";
import type { ProviderPreset } from "../../lib/protocol";
import Icon from "./../Icon";
import { Button, DialogShell, ExternalLink } from "./FormKit";

/**
 * Lưới các nhà cung cấp dựng sẵn.
 *
 * Mục đích không phải là tiết kiệm vài cú gõ base URL, mà là trả lời câu hỏi người dùng
 * thật sự đang hỏi: *cái nào gửi mã nguồn của tôi ra ngoài?* Nên hai huy hiệu quan trọng
 * nhất trên mỗi thẻ là "chạy trên máy này" và "cần khoá API", và chúng đứng trước cả mô
 * tả — không nằm dưới đáy thẻ như một chi tiết kỹ thuật.
 *
 * `onDevice` lấy thẳng từ preset chứ không đoán lại từ base URL: lõi đã tính rồi, và một
 * luật đoán thứ hai ở phía giao diện là một luật sẽ lệch đi sau lần sửa lõi đầu tiên.
 */
export default function PresetPicker(props: {
  presets: ProviderPreset[];
  onPick: (preset: ProviderPreset) => void;
  onManual: () => void;
  onClose: () => void;
}) {
  return (
    <DialogShell
      icon="model"
      title="Thêm nhà cung cấp mô hình"
      desc="Chọn một mục dựng sẵn, hoặc tự khai báo nếu máy chủ của bạn không có trong danh sách."
      wide
      onClose={props.onClose}
      footer={() => (
        <>
          <Button label="Tự khai báo…" variant="outline" icon="plus" onClick={props.onManual} />
          <span class="flex-1" />
          <Button label="Đóng" variant="ghost" onClick={props.onClose} />
        </>
      )}
    >
      <Show
        when={props.presets.length > 0}
        fallback={
          <p class="m-0 text-xs text-faint">
            Lõi chưa trả về mục dựng sẵn nào. Vẫn tự khai báo được bằng nút bên dưới.
          </p>
        }
      >
        <ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-sm p-0">
          <For each={props.presets}>
            {(preset) => (
              <li class="min-w-0">
                {/* Cả thẻ là một cái nút: nửa thẻ bấm được và nửa kia không là thứ chỉ
                    phát hiện ra bằng cách bấm trượt vài lần. Liên kết homepage nằm *bên
                    ngoài* nút, vì một <a> lồng trong <button> là HTML không hợp lệ và
                    trình đọc màn hình đọc ra hai thứ chồng nhau. */}
                <div class="flex h-full flex-col gap-2xs rounded-card border border-line bg-surface p-sm transition-colors duration-[var(--dur-fast)] hover:border-accent">
                  <button
                    type="button"
                    onClick={() => props.onPick(preset)}
                    class="flex min-w-0 flex-1 flex-col items-start gap-2xs text-left"
                  >
                    <span class="flex w-full min-w-0 items-center gap-2xs">
                      <span
                        class="grid size-6 shrink-0 place-items-center rounded-icon"
                        classList={{
                          "bg-accent-soft text-accent-ink": preset.onDevice,
                          "bg-[var(--overlay-faint)] text-muted": !preset.onDevice,
                        }}
                      >
                        <Icon name={preset.onDevice ? "plug" : "cloud"} size={13} />
                      </span>
                      <span class="min-w-0 flex-1 truncate text-xs font-semibold text-ink">
                        {preset.name}
                      </span>
                    </span>

                    <span class="flex flex-wrap gap-3xs">
                      <Show
                        when={preset.onDevice}
                        fallback={
                          <span class="inline-flex items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs text-warn">
                            <Icon name="cloud" size={10} />
                            Gửi ra ngoài
                          </span>
                        }
                      >
                        <span class="inline-flex items-center gap-3xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs font-medium text-accent-ink">
                          <Icon name="plug" size={10} />
                          Chạy trên máy này
                        </span>
                      </Show>
                      <span class="inline-flex items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                        <Icon name="key" size={10} />
                        {preset.needsKey ? "Cần khoá API" : "Không cần khoá"}
                      </span>
                    </span>

                    <span class="text-2xs text-muted">{preset.hint}</span>

                    {/* `defaultModel` chỉ là **gợi ý điền sẵn**, sửa được. Danh sách có
                        thẩm quyền đến từ `probe_provider` sau khi có base URL và khoá,
                        nên thẻ này không được đọc như một lựa chọn đã chốt. */}
                    <Show when={preset.defaultModel}>
                      {(model) => (
                        <span class="flex min-w-0 max-w-full items-baseline gap-2xs">
                          <span class="shrink-0 text-2xs text-faint">gợi ý</span>
                          <span class="min-w-0 truncate font-mono text-2xs text-faint">
                            {model()}
                          </span>
                        </span>
                      )}
                    </Show>
                  </button>

                  <div class="flex items-center justify-between gap-sm border-t border-line pt-2xs">
                    <span class="truncate font-mono text-2xs text-faint" title={preset.baseUrl}>
                      {preset.baseUrl}
                    </span>
                    <ExternalLink href={preset.homepage}>Trang chủ</ExternalLink>
                  </div>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </DialogShell>
  );
}
