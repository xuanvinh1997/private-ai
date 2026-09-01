import { Key } from "@solid-primitives/keyed";
import { formatBytes, formatLabel } from "../../lib/docs";
import type { DocumentView } from "../../lib/protocol";
import { relativeTime } from "../../lib/sessions";
import { IconButton } from "../primitives";
import EmbedBadge from "./EmbedBadge";

/**
 * Bảng tài liệu.
 *
 * Bảng thật (`<table>`) chứ không phải một chồng `div`: sáu cột dữ liệu cùng loại là
 * đúng định nghĩa của bảng, và trình đọc màn hình đọc được "Định dạng: PDF" thay vì đọc
 * một chuỗi từ rời rạc chỉ vì ta muốn dùng flexbox.
 *
 * Cuộn ngang nằm **trong khung của bảng**, không đẩy cả trang: một tiêu đề tài liệu dài
 * làm cả màn hình trượt ngang là cách nhanh nhất để mất chỗ đứng của thanh bên.
 */
export default function DocumentTable(props: {
  docs: DocumentView[];
  busy?: boolean;
  onRemove: (doc: DocumentView) => void;
}) {
  return (
    <div class="overflow-x-auto rounded-card border border-line bg-surface">
      <table class="w-full min-w-[720px] border-collapse text-left">
        <caption class="sr-only">Tài liệu trong thư viện</caption>
        <thead>
          <tr class="border-b border-line">
            <Th>Tài liệu</Th>
            <Th>Định dạng</Th>
            <Th>Kích thước</Th>
            <Th>Đoạn</Th>
            <Th>Nạp lúc</Th>
            <Th>Nhúng</Th>
            <th class="w-10 px-sm py-xs">
              <span class="sr-only">Thao tác</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {/* Keyed theo id: danh sách được thay nguyên mảng sau mỗi lần nạp, và keyed
              theo vị trí thì mọi hàng dựng lại — nút xoá đang có tiêu điểm biến mất
              dưới ngón tay người dùng ngay giữa lúc họ định bấm. */}
          <Key each={props.docs} by={(doc) => doc.id}>
            {(keyed) => (
              <tr class="border-b border-line last:border-0 transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-faint)]">
                <td class="max-w-[280px] px-sm py-xs align-top">
                  <span class="flex flex-col gap-3xs">
                    <span class="min-w-0 truncate text-xs text-ink" title={keyed().title}>
                      {keyed().title}
                    </span>
                    <span
                      class="min-w-0 truncate font-mono text-2xs text-faint"
                      dir="rtl"
                      title={keyed().path}
                    >
                      <bdi>{keyed().path}</bdi>
                    </span>
                  </span>
                </td>
                <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted">
                  {formatLabel(keyed().format)}
                </td>
                <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted tabular-nums">
                  {formatBytes(keyed().bytes)}
                </td>
                <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted tabular-nums">
                  {keyed().chunks}
                </td>
                <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted">
                  {relativeTime(keyed().addedAt)}
                </td>
                <td class="px-sm py-xs align-top">
                  <EmbedBadge doc={keyed()} />
                </td>
                <td class="px-sm py-xs align-top">
                  <IconButton
                    icon="trash"
                    size="sm"
                    danger
                    disabled={props.busy}
                    tip="left"
                    label={`Xoá "${keyed().title}" khỏi thư viện`}
                    onClick={() => props.onRemove(keyed())}
                  />
                </td>
              </tr>
            )}
          </Key>
        </tbody>
      </table>
    </div>
  );
}

function Th(props: { children: string }) {
  return (
    <th scope="col" class="px-sm py-xs text-2xs font-medium whitespace-nowrap text-faint">
      {props.children}
    </th>
  );
}
