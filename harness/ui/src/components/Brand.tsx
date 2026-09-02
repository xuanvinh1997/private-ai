/**
 * Dấu hiệu thương hiệu: một tấm khiên, khoét rỗng thành tia sáng.
 *
 * Trước đợt này ứng dụng **không có dấu hiệu nào** — chỗ đáng ra là logo chỉ có hai chữ
 * "Private AI" cỡ chữ bằng đúng mọi hàng điều hướng bên dưới nó. Một dòng chữ không lớn
 * hơn, không đậm hơn, không có hình gì đứng cạnh thì nó không đọc ra là tên sản phẩm; nó
 * đọc ra là một cái nhãn ai đó quên xoá.
 *
 * Hình vẽ nói đúng hai điều mà sản phẩm này bán: **khiên** là chuyện dữ liệu ở lại trên
 * máy, **tia sáng** là chuyện có mô hình bên trong. Hai ý đó là toàn bộ khác biệt của một
 * trợ lý chạy cục bộ so với một ô chat trên web, nên chúng đáng được nằm ở chỗ người dùng
 * nhìn đầu tiên.
 *
 * **Một path duy nhất, `evenodd`, `currentColor`.** Tia sáng là một lỗ thủng chứ không
 * phải một hình màu thứ hai, nên nó luôn đúng màu nền đứng sau nó — nền thanh bên, nền ô
 * nổi, hay nền sáng lẫn tối — mà không cần một token màu nào cho riêng nó. Một mảnh vẽ
 * hai màu thì phải nhớ đổi cả hai mỗi lần bảng màu đổi, và cái bị quên luôn là mảnh trong.
 */
export function BrandMark(props: { size?: number; class?: string }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      width={props.size ?? 24}
      height={props.size ?? 24}
      fill="currentColor"
      fill-rule="evenodd"
      class={props.class}
    >
      <path d="M12 2.5 4.5 5.4V11.5c0 4.5 3 8.6 7.5 9.9 4.5-1.3 7.5-5.4 7.5-9.9V5.4ZM12 6.8l1.15 3.05L16.2 11l-3.05 1.15L12 15.2l-1.15-3.05L7.8 11l3.05-1.15Z" />
    </svg>
  );
}

/**
 * Khiên cộng tên, dùng chung cho mọi chỗ cần xưng tên ứng dụng.
 *
 * Là một component chứ không phải hai dòng JSX chép đi chép lại: nó xuất hiện ở hai chỗ
 * có bố cục khác hẳn nhau — đầu thanh bên, và thanh trên khi thanh bên đã thu lại — và
 * hai bản chép tay sẽ lệch nhau về cỡ chữ ngay ở lần sửa thứ hai.
 *
 * `text-ink` cho phần chữ chứ không `text-accent`: tên sản phẩm là chữ, không phải một
 * nút bấm. Chỉ **khiên** mang màu nhấn, và đó cũng là chỗ duy nhất trên cả dải này có
 * màu — nên mắt rơi vào đúng nó.
 */
export function BrandLockup(props: { class?: string }) {
  return (
    <span class={`flex min-w-0 items-center gap-xs ${props.class ?? ""}`}>
      <BrandMark size={22} class="shrink-0 text-accent" />
      <span class="min-w-0 truncate text-base font-bold tracking-[-0.01em] text-ink">
        Private AI
      </span>
    </span>
  );
}
