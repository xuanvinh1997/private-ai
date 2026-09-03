import { createResource, For, Show } from "solid-js";
import { inTauri } from "../../lib/agent";
import { CopyButton } from "../primitives";
import { Banner, Row, RowGroup, SectionHead } from "./FormKit";
import { daVa, describeHarness, docCayPlugin, listHooks } from "./harness";

/**
 * Trang Hook — **chỉ đọc**, và cố ý dừng lại ở đó.
 *
 * Lõi chưa có lệnh nào liệt kê hook, chưa có lệnh nào thêm hay sửa hook. Dựng một biểu
 * mẫu gọi vào những lệnh chưa tồn tại thì được một màn hình đẹp mà mọi cú bấm đều ném lỗi
 * — tệ hơn hẳn một màn hình nói thẳng là chưa sửa được từ đây. Nên trang này làm đúng ba
 * việc: nói hook là gì, nói hai điều dễ hiểu nhầm nhất về nó, và chỉ ra tệp phải sửa.
 *
 * Hai điều dễ hiểu nhầm ấy đứng ở ngay đầu trang chứ không nằm cuối dưới dạng chú thích,
 * vì cả hai đều ngược với thứ người ta suy ra từ chữ "hook bảo mật": hook **fail-open**
 * (hỏng thì cho qua) trong khi hộp thoại duyệt fail-closed, và hook chạy **ngoài** vòng
 * giam trong khi mọi lệnh của trợ lý chạy trong đó.
 */

/**
 * Đường dẫn tệp cấu hình.
 *
 * Chuỗi cứng, và nó có thể sai: lõi đọc `PAI_DATA_DIR` trước khi lùi về `~/.private-ai`.
 * Giao diện không hỏi được biến môi trường của tiến trình lõi, nên chỗ này nói ra cả hai
 * thay vì hiện một đường dẫn chắc nịch mà người dùng có thể không tìm thấy.
 */
const TEP_VA = "~/.private-ai/patch.yaml";

/** Mẫu để chép thẳng vào tệp vá. Một hàng `replace` chứ không phải `insert`: hàng `hooks`
 *  đã có sẵn trong cây dựng sẵn với danh sách rỗng, nên thêm lần nữa sẽ dừng khởi động. */
const MAU = `patches:
  - op: replace
    id: hooks
    config:
      hooks:
        - command: "jq -e '.arguments.command | test(\\"rm -rf\\") | not' >/dev/null && echo '{\\"decision\\":\\"allow\\"}' || echo '{\\"decision\\":\\"deny\\",\\"reason\\":\\"khong chay rm -rf\\"}'"
          tools: ["bash"]
          timeout_secs: 5`;

export default function HooksPage() {
  // Cây plugin trả lời được đúng một câu về hook, và đó là câu đáng giá nhất ở đây: hàng
  // `hooks` có bị một lớp cấu hình của người dùng vá vào không. Không thì chắc chắn chưa
  // có hook nào, vì bản dựng sẵn khai `hooks: []`.
  const [cay] = createResource(async () => docCayPlugin(await describeHarness()));
  // Danh sách hook thật, đọc từ chính hàng cấu hình đã áp lớp. Rỗng là mặc định, không
  // phải lỗi: bản dựng sẵn khai `hooks: []` và phần lớn người dùng không đụng tới nó.
  const [hooks] = createResource(listHooks);

  /**
   * Một chỗ duy nhất quyết định cả câu mô tả lẫn nhãn ở cột phải.
   *
   * Bốn trạng thái (không có lõi, đang hỏi, hỏi hỏng, đọc được) mà tách ra hai chuỗi
   * `?:` lồng nhau thì hai chuỗi ấy sẽ lệch nhau ở lần sửa thứ hai — và lệch ở đây nghĩa
   * là nhãn nói "rỗng" trong khi câu dưới nói "đã bị vá".
   */
  const trangThai = (): {
    nhan: string;
    moTa: string;
    them?: string;
    tone: "faint" | "ok" | "muted";
  } => {
    if (!inTauri()) return { nhan: "—", moTa: "Bản demo không có lõi để hỏi.", tone: "faint" };
    if (cay.loading) return { nhan: "đang hỏi…", moTa: "Đang hỏi lõi…", tone: "faint" };
    if (cay.error !== undefined)
      return { nhan: "lỗi", moTa: "Không hỏi được lõi.", tone: "faint" };
    const row = cay()?.find((item) => item.id === "hooks");
    if (row === undefined)
      return {
        nhan: "vắng",
        moTa: "Không có hàng `hooks` trong cây đang chạy.",
        them: "Không có hàng `hooks` nào trong cây đang chạy, nên không hook nào chạy được.",
        tone: "muted",
      };
    if (!daVa(row))
      return {
        nhan: "rỗng",
        moTa: "Vẫn như bản dựng sẵn: danh sách hook rỗng.",
        them: "Vẫn đúng như bản dựng sẵn, tức là danh sách hook rỗng. Chưa có hook nào chạy trên máy này.",
        tone: "muted",
      };
    return {
      nhan: "có vá",
      moTa: `Đã bị một lớp cấu hình vá vào: ${row.origin}.`,
      them: `Đã bị một lớp cấu hình của bạn vá vào: ${row.origin}. Nghĩa là tệp vá có khai hook — nhưng khai những gì thì lệnh chẩn đoán không nói ra.`,
      tone: "ok",
    };
  };

  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          icon="warn"
          title="Ba điều phải biết trước"
          desc="Cả ba đều ngược với chữ “hook bảo mật”."
        />

        <RowGroup>
          <Row
            icon="warn"
            label="Hook hỏng thì cho qua"
            desc="Hook lỗi thì lời gọi vẫn chạy."
            more="Hook lỗi cú pháp, hết giờ hay thiếu tệp đều là lỗi của chính sách, không phải bằng chứng rằng lời gọi nguy hiểm — nên lời gọi vẫn chạy. Hộp thoại duyệt thì ngược lại: không trả lời được là từ chối."
            control={() => <span class="text-2xs text-warn">fail-open</span>}
          />
          <Row
            icon="shield"
            label="Hook chạy ngoài vòng giam"
            desc="Hook chạy với đầy đủ quyền của bạn."
            more="Hook được spawn thẳng, không qua seam Shell, nên nó chạy với đầy đủ quyền của bạn. Để vòng giam của trợ lý quyết định chính sách có được chạy hay không là lộn ngược quan hệ."
            control={() => <span class="text-2xs text-warn">đầy đủ quyền</span>}
          />
          <Row
            icon="hand"
            label="Hook không sửa được tham số"
            desc="Chỉ allow hoặc deny, không viết lại tham số."
            more="Chỉ allow hoặc deny. Viết lại tham số nghe tiện, nhưng nó tạo ra một lời gọi mà cả mô hình lẫn bạn đều không thấy, và bản ghi sẽ nói dối về thứ đã chạy."
            control={() => <span class="text-2xs text-faint">chỉ chặn</span>}
          />
        </RowGroup>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          icon="list"
          title="Hook đang cài"
          desc="Đọc từ hàng cấu hình đã áp lớp."
          more="Đọc từ hàng cấu hình đã áp lớp — lệnh, tool nó áp vào, và hạn giờ riêng nếu có."
        />

        <Show
          when={(hooks() ?? []).length > 0}
          fallback={
            <RowGroup>
              <Row
                icon="check"
                label="Chưa cài hook nào"
                desc="Đây là mặc định, không gì chen vào giữa."
                more="Đây là mặc định. Mỗi hook là một lệnh chạy trước mỗi lời gọi tool, nên không có hook nghĩa là không có gì chen vào giữa."
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
                  desc={`${
                    hook.tools.length === 0
                      ? "Áp cho mọi tool"
                      : `Chỉ áp cho: ${hook.tools.join(", ")}`
                  } · hạn giờ ${hook.timeoutSecs ?? 10} giây · khai ở ${hook.origin}`}
                />
              )}
            </For>
          </RowGroup>
        </Show>

        <RowGroup>
          <Row
            icon="plug"
            label="Hàng `hooks` trong cây plugin"
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
          title="Đọc được, chưa sửa được từ đây"
          more="Lõi đã liệt kê được hook đang cài, nhưng chưa có lệnh nào thêm, sửa hay xoá — nên màn hình này không dựng biểu mẫu. Một biểu mẫu gọi vào lệnh không tồn tại thì mọi cú bấm đều ném lỗi. Cho tới lúc có lệnh ấy, hook cấu hình bằng cách sửa tay tệp vá."
        >
          Cấu hình hook bằng cách sửa tay tệp vá.
        </Banner>
      </section>

      <section class="flex flex-col gap-md">
        <SectionHead
          icon="pencil"
          title="Sửa bằng tay"
          desc="Sửa xong phải mở lại ứng dụng."
          more="Sửa xong phải mở lại ứng dụng: cây plugin được dựng một lần lúc khởi động."
        />

        <RowGroup>
          <Row
            icon="document"
            label={TEP_VA}
            labelMono
            desc="Chỗ duy nhất khai được hook."
            more="Chỗ duy nhất khai được hook. Đây là đường dẫn mặc định — đặt biến môi trường PAI_DATA_DIR thì tệp nằm trong thư mục đó."
            control={() => <CopyButton text={() => TEP_VA} label="Chép đường dẫn tệp vá" />}
          />
          <Row
            icon="code"
            label="Trường của một hook"
            desc="Ba trường: command, tools, timeout_secs."
            more="command chạy qua /bin/sh -c. tools là danh sách tool nó áp vào, rỗng nghĩa là mọi tool. timeout_secs là hạn giờ riêng, vắng thì lấy mặc định 10 giây."
          />
        </RowGroup>

        <div class="flex flex-col gap-2xs">
          <div class="flex items-center justify-between gap-sm">
            <span class="text-2xs text-faint">Mẫu một hook chặn `rm -rf` cho tool bash</span>
            <CopyButton text={() => MAU} label="Chép mẫu cấu hình hook" />
          </div>
          <pre class="m-0 overflow-x-auto rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-sm font-mono text-2xs text-text">
            {MAU}
          </pre>
        </div>
      </section>
    </div>
  );
}
