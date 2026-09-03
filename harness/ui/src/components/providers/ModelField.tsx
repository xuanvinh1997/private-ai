import { createEffect, createSignal, on, Show } from "solid-js";
import type { ModelChoice } from "../../lib/protocol";
import { usableForChat } from "../ModelPicker";
import { InfoDot, Select, TextField } from "../settings/FormKit";

/**
 * Một ô chọn mô hình, dùng cho **cả hai vai**: hội thoại và nhúng.
 *
 * Một thành phần chứ không phải hai, vì hai vai hỏi cùng một câu — *máy chủ này có mô hình
 * nào, và tôi lấy cái nào* — và trước đây hộp thoại provider trả lời nó bằng ba hình dạng
 * khác nhau trong cùng một cột: một ô gõ tay cho tên mô hình hội thoại, một hộp cuộn đầy
 * nút bấm ngay dưới nó, và một ô gõ tay nữa cho mô hình nhúng. Ba hình dạng cho một câu
 * hỏi là ba thứ phải học, và cái hộp cuộn còn ăn mất chiều cao của hộp thoại đúng lúc
 * người dùng cần nhìn thấy cả biểu mẫu.
 *
 * Bốn luật, và cả bốn đều là luật về sự thành thật:
 *
 *   1. **Danh sách là thứ tự, không phải bộ lọc.** Cờ năng lực chỉ có thẩm quyền ở LM
 *      Studio; hai loại provider kia chỉ cho một cái tên và lõi đoán từ đó. Đoán trượt thì
 *      mô hình vẫn còn trong danh sách, chỉ nằm dưới và mang một chữ nói rõ vì sao — chứ
 *      không biến mất.
 *   2. **Luôn còn lối gõ tay.** Một máy chủ tự vận hành có thể phục vụ đúng một mô hình và
 *      chẳng buồn liệt kê, và một ô chọn rỗng khi đó là một màn hình không cấu hình được.
 *   3. **Không tự sửa giá trị đang có**, kể cả khi tên đang lưu không nằm trong danh sách.
 *   4. **Không dán nhãn `tools`.** Cờ đó trong kết quả thử gần như luôn là phỏng đoán, và
 *      một cảnh báo luôn bật là một cảnh báo không ai đọc nữa. Nó thuộc về bộ chọn mô hình
 *      ở trang danh sách provider, nơi số liệu đến từ `list_models`.
 */

/** Mô hình máy chủ khai là nhúng được — dùng để xếp lên đầu, không dùng để lọc. */
export const embeddable = (model: ModelChoice) => model.embedding;

/**
 * Hai tên có chỉ cùng một mô hình không.
 *
 * Bỏ đuôi `:latest` vì Ollama **liệt kê** kèm đuôi (`nomic-embed-text:latest`) nhưng
 * **nhận** cả tên trần, và tên người dùng gõ — hay hàng gieo lúc cài đặt — thì không có
 * đuôi. So thẳng chuỗi ở đây cho ra hai hậu quả, cả hai đều tệ: một cảnh báo "máy chủ
 * không có mô hình này" bật lên cho đúng cấu hình mặc định, và một cú bấm Lưu nhúng lại
 * cả thư viện để đổi sang chính mô hình đang dùng.
 */
export const sameModel = (left: string, right: string) => {
  const bare = (value: string) => value.trim().toLowerCase().replace(/:latest$/, "");
  return bare(left) === bare(right);
};

/** Tên đang đặt mà máy chủ không khai. Chỉ hỏi được khi có danh sách để mà đối chiếu. */
export const notListed = (models: ModelChoice[], value: string) =>
  models.length > 0 && value.trim() !== "" && !models.some((entry) => sameModel(entry.id, value));

/**
 * Mục "gõ tên khác" trong ô chọn.
 *
 * Một dấu ngoặc nhọn ở đầu để không đụng tên mô hình thật: id mô hình có thể chứa gần như
 * mọi ký tự, nhưng không có máy chủ nào đặt tên bắt đầu bằng `<`.
 */
const CUSTOM = "<custom>";

export type ModelRole = "chat" | "embedding";

/** Mô hình nào hợp vai, và mô hình lạc vai thì mang chữ gì. */
const SORT: Record<ModelRole, { fits: (model: ModelChoice) => boolean; otherwise: string }> = {
  chat: { fits: usableForChat, otherwise: "chỉ nhúng được" },
  embedding: { fits: embeddable, otherwise: "không khai là nhúng được" },
};

export default function ModelField(props: {
  role: ModelRole;
  /** Tên của ô, luôn tới được trình đọc màn hình dù có vẽ ra hay không. */
  label: string;
  /** Vẽ nhãn ra. Tắt khi ô nằm trong một `<Row>` đã mang nhãn ở cột trái. */
  showLabel?: boolean;
  hint?: string;
  more?: string;
  models: ModelChoice[];
  value: string;
  placeholder?: string;
  disabled?: boolean;
  onInput: (value: string) => void;
}) {
  /**
   * Người dùng **cố ý** chọn "gõ tên khác".
   *
   * Tách khỏi trường hợp tên-không-có-trong-danh-sách bên dưới: cái sau tự suy ra được từ
   * props, và giữ nó thành trạng thái nữa là dựng hai nguồn sự thật cho cùng một câu hỏi.
   */
  const [chose, setChose] = createSignal(false);

  // Đổi máy chủ là đổi câu hỏi. Giữ lại "đang gõ tay" qua một lần đổi danh sách thì ô chọn
  // của máy chủ mới không bao giờ hiện ra, và không có gì trên màn hình nói vì sao.
  createEffect(on(() => props.models, () => setChose(false), { defer: true }));

  const typing = () => chose() || notListed(props.models, props.value);

  /**
   * Tên **như máy chủ liệt kê**, để ô chọn trỏ đúng mục.
   *
   * Không ghi ngược giá trị ấy ra ngoài: đổi `nomic-embed-text` thành
   * `nomic-embed-text:latest` sau lưng người dùng làm biểu mẫu bẩn, và ở màn hình mô hình
   * nhúng thì cú Lưu sau đó nhúng lại cả thư viện để đổi sang đúng mô hình đang chạy.
   */
  const listed = () =>
    props.models.find((entry) => sameModel(entry.id, props.value))?.id ?? props.value;

  const options = () => {
    const rule = SORT[props.role];
    const label = (model: ModelChoice, fits: boolean) => {
      // Cửa sổ ngữ cảnh chỉ đi cùng vai hội thoại: ở đó nó là thứ người ta cân nhắc khi
      // chọn, còn cạnh một mô hình nhúng thì nó là một con số không dùng vào việc gì.
      const size =
        props.role === "chat" && model.contextWindow !== null
          ? ` · ${Intl.NumberFormat("vi-VN").format(model.contextWindow)} token`
          : "";
      return `${model.id}${size}${fits ? "" : ` · ${rule.otherwise}`}`;
    };
    return [
      ...props.models
        .filter(rule.fits)
        .map((entry) => ({ id: entry.id, label: label(entry, true) })),
      ...props.models
        .filter((entry) => !rule.fits(entry))
        .map((entry) => ({ id: entry.id, label: label(entry, false) })),
      { id: CUSTOM, label: "Gõ tên khác…" },
    ];
  };

  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      {/* Nhãn là một `<span>`, không phải `<label for>`: ô nó trỏ tới đổi giữa `<select>`
          và `<input>` theo câu trả lời của máy chủ, và một `for` trỏ vào phần tử vừa biến
          mất còn tệ hơn không có `for`. Cả hai ô đều mang `aria-label` riêng, nên trình
          đọc màn hình vẫn nghe đúng tên. */}
      <Show when={props.showLabel === true}>
        <span class="flex items-center gap-2xs text-2xs text-faint">
          {props.label}
          <Show when={props.more}>
            {(text) => <InfoDot text={text()} label={`Về ${props.label}`} />}
          </Show>
        </span>
      </Show>

      <Show when={props.models.length > 0}>
        <Select
          label={props.label}
          mono
          full
          value={typing() ? CUSTOM : listed()}
          options={options()}
          disabled={props.disabled}
          onPick={(value) => {
            if (value === CUSTOM) {
              setChose(true);
              return;
            }
            setChose(false);
            props.onInput(value);
          }}
        />
      </Show>

      {/* Ô gõ tay hiện khi chưa có danh sách nào, khi tên đang lưu không nằm trong danh
          sách, hoặc khi người dùng cố ý chọn "gõ tên khác". Ba lối vào, một ô — vì cùng
          là một việc. Nó có mặt ngay từ lúc màn hình mở ra, trước khi máy chủ kịp trả
          lời: một ô trống hiện lên sau một giây chờ đọc như một ô vừa bị xoá. */}
      <Show when={props.models.length === 0 || typing()}>
        <TextField
          label={props.label}
          hideLabel
          mono
          value={props.value}
          disabled={props.disabled}
          placeholder={props.placeholder}
          onInput={props.onInput}
        />
      </Show>

      {/* Tên không có trong kho máy chủ, nói ra **chỉ ở vai nhúng**. Ở vai hội thoại, một
          cái tên sai vỡ ngay ở tin nhắn đầu tiên và tự nó nói ra; ở vai nhúng thì biểu mẫu
          trông đầy đủ và mỗi lần nạp tài liệu thất bại ở một chỗ không ai đang nhìn. Hay
          gặp nhất là dán tên kho HuggingFace (`nomic-ai/nomic-embed-text-v1.5-GGUF`) trong
          khi máy chủ nhận một id khác hẳn. */}
      <Show when={props.role === "embedding" && notListed(props.models, props.value)}>
        <p class="m-0 flex items-center gap-2xs text-2xs text-warn">
          Máy chủ không khai mô hình này.
          <InfoDot
            label="Về tên mô hình không có trong danh sách"
            text="Danh sách là cái máy chủ tự khai. Tên không nằm trong đó vẫn có thể chạy — một máy chủ tự vận hành thường phục vụ đúng một mô hình và chẳng buồn liệt kê — nên đây là một lời nhắc, không phải một cái chặn. Muốn biết chắc thì mở màn hình Mô hình nhúng và bấm “Thử ngay”: nó gửi thật một câu đi và đo vector nhận về."
          />
        </p>
      </Show>

      <Show when={props.hint}>{(hint) => <p class="m-0 text-2xs text-faint">{hint()}</p>}</Show>
    </div>
  );
}
