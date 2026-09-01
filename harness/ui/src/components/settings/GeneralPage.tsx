import { displayMode, setDisplayMode, type DisplayMode } from "../../lib/prefs";
import { setTheme, theme, type ThemeChoice } from "../../lib/theme";
import { Row, RowGroup, SectionHead, Select } from "./FormKit";

/**
 * Trang Chung: hai thứ duy nhất đổi được mà không đụng tới lõi.
 *
 * Cả hai đều là hàng có ô chọn ở mép phải, không phải dãy nút bo tròn như bản trước. Dãy
 * nút đọc nhanh hơn khi có ba lựa chọn, nhưng nó là **kiểu hàng thứ hai** trong một màn
 * hình mà sáu trang còn lại đều dùng hàng-có-control-bên-phải, và hai kiểu hàng cạnh nhau
 * đọc ra là hai màn hình bị dán vào nhau. Một màn hình cài đặt nhất quán đáng giá hơn một
 * cú bấm tiết kiệm được ở đúng hai hàng.
 */
export default function GeneralPage() {
  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          title="Hiển thị"
          desc="Đổi là thấy ngay, và được nhớ lại ở lần mở sau."
        />
        <RowGroup>
          <Row
            label="Bảng màu"
            desc="Theo hệ thống thì cửa sổ đổi màu cùng lúc với macOS, kể cả khi đang mở."
            control={() => (
              <Select
                label="Bảng màu"
                value={theme()}
                onPick={(value) => setTheme(value as ThemeChoice)}
                options={[
                  { id: "light", label: "Sáng" },
                  { id: "dark", label: "Tối" },
                  { id: "system", label: "Theo hệ thống" },
                ]}
              />
            )}
          />
          <Row
            label="Cách hiển thị hội thoại"
            desc="Chế độ tài liệu bỏ bong bóng và trải hết bề rộng — dễ đọc hơn với diff dài và bảng rộng."
            control={() => (
              <Select
                label="Cách hiển thị hội thoại"
                value={displayMode()}
                onPick={(value) => setDisplayMode(value as DisplayMode)}
                options={[
                  { id: "bubble", label: "Bong bóng" },
                  { id: "document", label: "Tài liệu" },
                ]}
              />
            )}
          />
        </RowGroup>
      </section>
    </div>
  );
}
