import { createResource, For, Show } from "solid-js";
import { inTauri } from "../../lib/agent";
import { defaultToolScope, setDefaultToolScope } from "../../lib/prefs";
import type { ToolScope } from "../../lib/protocol";
import { Disclosure } from "../primitives";
import { Banner, Row, RowGroup, SectionHead, Select } from "./FormKit";
import { describeHarness, docCayPlugin, sandboxStatus } from "./harness";

/**
 * Trang Quyền.
 *
 * Hai mục, và ranh giới giữa chúng là ranh giới quan trọng nhất của cả trang: **mục trên
 * đổi được, mục dưới chỉ đọc**. Mức phạm vi là quyết định của người dùng; vòng giam là sự
 * thật về máy đang chạy, và một sự thật về máy mà giao diện cho bấm vào là một lời hứa
 * giao diện không giữ được.
 *
 * Câu mô tả của mức `Chạy lệnh` nói thẳng hậu quả thay vì nói "cấp thêm quyền". Nó có tác
 * dụng đúng một lần — lúc người ta đọc trước khi đổi — và một câu tử tế chung chung ở
 * đúng chỗ đó là cách chắc chắn nhất để người ta bấm qua mà không nhớ mình vừa bật gì.
 */

const NHAN: Record<ToolScope, string> = {
  read: "Chỉ đọc",
  write: "Đọc và ghi",
  shell: "Đọc, ghi và chạy lệnh",
};

/**
 * Hậu quả của từng mức, viết theo thứ tự *cái gì mở ra* rồi *cái gì vẫn đóng*.
 *
 * Nói cả phần vẫn đóng là cố ý: một câu chỉ liệt kê quyền mới mở đọc ra như một lời cảnh
 * báo, và người dùng học rất nhanh cách bỏ qua lời cảnh báo. Nói rõ ranh giới thì họ chọn
 * được thay vì chỉ đồng ý.
 */
const HAU_QUA: Record<ToolScope, string> = {
  read: "Trợ lý đọc được tệp và tìm trong dự án, và chỉ thế. Nó không sửa được tệp nào và không chạy được lệnh nào.",
  write:
    "Trợ lý đọc và sửa được tệp trong thư mục dự án. Nó vẫn không chạy được lệnh nào trên máy này.",
  shell:
    "Trợ lý được thi hành lệnh trên máy này — build, cài gói, xoá tệp — và mỗi lệnh chạy dưới đúng tài khoản của bạn, với đúng quyền của bạn.",
};

/** Câu trả lời cho "hàng `sandbox` có đang cắm không", kèm màu để đọc lướt. */
type TrangThai = { text: string; tone: "faint" | "ok" | "warn" | "danger" };

/** Nhãn mức giam, và nó nói gì. Ba mức, ba câu khác nhau — gộp lại là mất đúng thông tin. */
const MUC_GIAM: Record<string, { nhan: string; tone: "ok" | "warn" | "danger" }> = {
  full: { nhan: "Đầy đủ", tone: "ok" },
  partial: { nhan: "Một phần", tone: "warn" },
  none: { nhan: "Không giam", tone: "danger" },
};

export default function PermissionsPage() {
  // Hỏi lõi mức giam thật. `null` nghĩa là **không hỏi được**, khác hẳn `none` nghĩa là
  // **không có vòng giam** — câu thứ hai là một khẳng định về an toàn, câu thứ nhất thì
  // không, và hiện nhầm câu này thành câu kia là nói dối theo hướng nguy hiểm.
  const [giam] = createResource(sandboxStatus);

  // Cây plugin, hỏi để trả lời **một** câu: vòng giam có đang được cắm không. Hỏi mỗi lần
  // mở trang chứ không nhớ lại, vì cây được dựng lại theo dự án đang mở.
  const [cay] = createResource(async () => docCayPlugin(await describeHarness()));

  const sandbox = (): TrangThai => {
    if (!inTauri()) return { text: "không có lõi ở bản demo", tone: "faint" };
    if (cay.loading) return { text: "đang hỏi…", tone: "faint" };
    if (cay.error !== undefined) return { text: "không hỏi được lõi", tone: "danger" };
    const row = cay()?.find((item) => item.id === "sandbox");
    if (row === undefined) return { text: "không có trong cây", tone: "warn" };
    if (row.disabled) return { text: "đang bị tắt", tone: "warn" };
    return { text: "có", tone: "ok" };
  };

  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          title="Quyền mặc định"
          desc="Mức mà một lượt mới bắt đầu ở đó. Từng lượt vẫn đổi được trong ô soạn tin."
        />
        <RowGroup>
          <Row
            label="Phạm vi tool cho lượt mới"
            desc={HAU_QUA[defaultToolScope()]}
            control={() => (
              <Select
                label="Phạm vi tool cho lượt mới"
                value={defaultToolScope()}
                onPick={(value) => setDefaultToolScope(value as ToolScope)}
                options={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
                  id: scope,
                  label: NHAN[scope],
                }))}
              />
            )}
            below={() => (
              <Show when={defaultToolScope() === "shell"}>
                <Banner tone="warn" icon="warn" title="Mức này mở lệnh shell ngay từ lượt đầu">
                  Vòng giam chỉ chặn phần <b>ghi ra ngoài thư mục dự án</b>. Nó không chặn
                  mạng và không chặn việc đọc: một lệnh vẫn tải được mọi thứ về, vẫn đọc
                  được khoá nằm trong <code class="font-mono">~/.ssh</code>, và vẫn gửi
                  được mọi thứ đi. Thứ duy nhất còn đứng chắn là hộp thoại duyệt — nó hỏi
                  trước mỗi lệnh, và nó nói luôn vòng giam trên máy này đang ở mức nào.
                </Banner>
              </Show>
            )}
          />
          <Row
            label="Bộ chọn trong ô soạn tin"
            desc="Vẫn là của từng lượt. Thiết lập ở trên chỉ quyết định lượt mới mở ra ở mức nào; đổi trong ô soạn tin không ghi đè lại nó."
          />
        </RowGroup>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          title="Vòng giam tiến trình"
          desc="Chỉ đọc. Vòng giam là sự thật về máy đang chạy, không phải một tuỳ chọn."
        />

        <RowGroup>
          <Row
            label="Mức giam trên máy này"
            desc={
              giam()?.reason ??
              "Kernel thi hành đúng cái đã khai: ghi ra ngoài vùng cho phép là thất bại, không phải là “thường thì thất bại”."
            }
            control={() => (
              <Show
                when={giam()}
                fallback={<span class="text-2xs text-faint">chưa hỏi được lõi</span>}
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
                    {MUC_GIAM[muc().mode]?.nhan ?? muc().mode}
                  </span>
                )}
              </Show>
            )}
          />
          <Show when={giam()?.writableRoots.length}>
            <Row
              label="Thư mục ghi được"
              desc="Lệnh chỉ ghi được vào đây. Mọi chỗ khác trên đĩa là chỉ đọc — nhưng đọc thì không bị chặn ở đâu cả."
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
            label="Vòng giam chặn gì"
            desc="Chỉ hiệu ứng ghi lên tệp, và chỉ phần nằm ngoài thư mục dự án. Không chế độ nào chặn mạng, và cả ba chế độ đều cho đọc toàn máy."
          />
          <Row
            label="Vòng giam đang được cắm"
            desc="Hàng `sandbox` có trong cây plugin đang chạy hay không. Có mặt vẫn chưa chắc giam được: nơi chưa hỗ trợ thì nó cắm rồi tự khai là không giam."
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
                {sandbox().text}
              </span>
            )}
          />
        </RowGroup>

        <Show when={(cay()?.length ?? 0) > 0}>
          {/* Bản in nguyên văn của lõi, không diễn giải lại. Đây là câu trả lời cho "bản
              đang chạy thật sự gồm những gì" — câu hỏi đầu tiên khi có gì đó sai — và một
              bản tóm tắt gọn gàng ở chỗ này sẽ giấu mất đúng cái hàng đang gây chuyện. */}
          <Disclosure label="Cây plugin đang chạy" hint={`${cay()?.length ?? 0} hàng`}>
            <ul class="m-0 flex list-none flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-sm">
              <For each={cay()}>
                {(row) => (
                  <li class="flex flex-wrap items-baseline gap-2xs font-mono text-2xs">
                    <span class="text-ink">{row.id}</span>
                    <span class="text-muted">{row.plugin}</span>
                    <Show when={row.disabled}>
                      <span class="text-warn">[tắt]</span>
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
