import { createSignal, onMount } from "solid-js";
import type { IconName } from "./Icon";
import DialogShell, { Button } from "./projects/DialogShell";

/**
 * Hỏi **một dòng chữ**, và không hỏi gì thêm.
 *
 * Dựng trên `DialogShell` chứ không tự vẽ lại cái khung: ba thứ vô hình của một hộp thoại
 * — bẫy tiêu điểm, Esc đóng, trả tiêu điểm về chỗ cũ — đã nằm sẵn trong đó, và viết lại
 * chúng ở đây là mở thêm một chỗ để quên một trong ba.
 *
 * Cái nó thêm vào so với hộp nhập của trình duyệt không phải là màu sắc: giá trị cũ được
 * **bôi đen sẵn**, nên gõ đè là xong còn sửa một chữ cũng vẫn được; nút xác nhận mang tên
 * việc sắp làm chứ không phải "OK"; và ô trống sau khi trim thì nút tắt hẳn, thay vì nhận
 * một cái tên rỗng rồi để chỗ gọi tự đoán xem người dùng muốn gì.
 */
export default function PromptDialog(props: {
  title: string;
  desc?: string;
  /** Nhãn của ô nhập. Nói về *giá trị*, không lặp lại tiêu đề. */
  label: string;
  /** Giá trị mở sẵn — thường là giá trị đang có, chứ không phải ô trống. */
  value: string;
  placeholder?: string;
  confirmLabel: string;
  icon?: IconName;
  onConfirm: (value: string) => void;
  onClose: () => void;
}) {
  let input: HTMLInputElement | undefined;
  const [value, setValue] = createSignal(props.value);
  const trimmed = () => value().trim();

  const submit = () => {
    if (trimmed() === "") return;
    props.onConfirm(trimmed());
  };

  onMount(() => {
    // Hoãn một nhịp vi tác vụ: vỏ hộp thoại tự đưa tiêu điểm vào phần tử focus được đầu
    // tiên — đúng cái ô này — nhưng nó chỉ focus chứ không bôi đen, và nó chạy *sau* đoạn
    // này. Bôi đen ngay ở đây thì cú focus của vỏ đè lên và người dùng nhận được một cái
    // tên cũ nằm nguyên trong ô, phải xoá tay từng chữ mới gõ được tên mới.
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  return (
    <DialogShell
      icon={props.icon ?? "pencil"}
      title={props.title}
      desc={props.desc}
      onClose={props.onClose}
      footer={() => (
        <>
          <Button onClick={props.onClose}>Huỷ</Button>
          <Button variant="primary" onClick={submit} disabled={trimmed() === ""}>
            {props.confirmLabel}
          </Button>
        </>
      )}
    >
      <label class="flex flex-col gap-2xs">
        <span class="text-xs text-muted">{props.label}</span>
        <input
          ref={input}
          type="text"
          value={value()}
          placeholder={props.placeholder}
          spellcheck={false}
          autocapitalize="off"
          autocomplete="off"
          onInput={(event) => setValue(event.currentTarget.value)}
          onKeyDown={(event) => {
            // Enter xác nhận — một dòng chữ thì không có lý do bắt người ta rời tay khỏi
            // bàn phím đi tìm cái nút. `preventDefault` để phím ấy chỉ mang đúng một
            // nghĩa, không kèm theo một hành vi mặc định nào của trình duyệt.
            if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
          class="h-(--cta-h) min-w-0 rounded-btn border border-line bg-bg px-sm text-sm text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        />
      </label>
    </DialogShell>
  );
}
