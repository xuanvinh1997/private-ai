import { createEffect, createSignal, onMount, Show } from "solid-js";
import { rerankSetting, setRerank } from "../../lib/providers";
import type { RerankSetting } from "../../lib/protocol";
import {
  Banner,
  Row,
  RowGroup,
  SectionHead,
  Select,
  TextField,
  Toggle,
} from "../settings/FormKit";

/**
 * Mục xếp hạng lại, nằm cuối trang Mô hình.
 *
 * # Vì sao nó ở đây chứ không phải một trang riêng
 *
 * Nó là mô hình **thứ ba** trên đường truy hồi, sau mô hình nhúng và trước mô hình trả
 * lời. Trang này đã gom hai cái kia, và tách cái thứ ba ra buộc người dùng đi qua ba
 * trang để hiểu một đường đi.
 *
 * # Vì sao nó không nằm trong danh sách nhà cung cấp
 *
 * Vì mặc định nó **không phải một máy chủ**: đó là một tệp mô hình tải từ Hugging Face và
 * chạy trong tiến trình đọc tài liệu. Không có địa chỉ, không có khoá, không thử kết nối
 * được. Đặt nó thành một hàng trong danh sách máy chủ sẽ cho ba ô trống không có nghĩa.
 *
 * # Ba thứ màn hình này phải nói ra
 *
 *   1. **Tắt đi thì mất gì.** Không phải "mất tính năng" chung chung: truy hồi vẫn chạy,
 *      chỉ là thứ tự kém hơn ở những câu hỏi cần hiểu nghĩa thay vì khớp từ.
 *   2. **Bật lên thì chậm bao nhiêu.** Con số phụ thuộc số ứng viên và việc service có
 *      GPU hay không — đo được là chênh khoảng mười lần. Không nói ra thì người dùng chỉ
 *      thấy "tìm kiếm chậm" và không có đường nào lần ra nguyên nhân.
 *   3. **Đổi ở đây không nhúng lại gì.** Khác hẳn mục mô hình nhúng ngay phía trên, nơi
 *      đổi một ô là chạy lại cả thư viện. Người vừa đọc cảnh báo đó cần biết nó không áp
 *      dụng ở đây.
 */
/**
 * `TextField` chốt khi rời ô hoặc khi bấm Enter, không chốt theo từng phím.
 *
 * `FormKit.TextField` chỉ có `onInput`, và lưu theo từng phím thì gõ "30" sẽ lưu "3"
 * trước — mà lõi siết `topN` không vượt quá `candidates`, nên nó sẽ nắn giá trị ngay giữa
 * lúc người dùng còn đang gõ. Ô số nhảy dưới tay là kiểu hỏng khiến người ta thôi không
 * chỉnh nữa.
 *
 * Bản nháp sống ở đây; giá trị thật chỉ đi lên khi người dùng đã gõ xong.
 */
function CommitField(props: {
  label: string;
  value: string;
  disabled?: boolean;
  mono?: boolean;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = createSignal(props.value);
  // Giá trị từ lõi về — kể cả giá trị nó vừa siết lại — phải ghi đè bản nháp. Không có
  // dòng này thì ô hiện mãi thứ người dùng gõ, còn kho đã lưu một số khác.
  createEffect(() => setDraft(props.value));

  const commit = () => {
    if (draft() !== props.value) props.onCommit(draft());
  };

  return (
    <TextField
      label={props.label}
      hideLabel
      mono={props.mono}
      value={draft()}
      disabled={props.disabled}
      onInput={setDraft}
      ref={(el) => {
        el.addEventListener("blur", commit);
        el.addEventListener("keydown", (event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
            el.blur();
          }
        });
      }}
    />
  );
}

export default function RerankView() {
  const [setting, setSetting] = createSignal<RerankSetting | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => setSetting(await rerankSetting()));

  /** Lưu một thay đổi, rồi vẽ lại theo **cái lõi trả về**, không theo cái vừa gửi đi. */
  async function save(patch: Partial<Omit<RerankSetting, "reason">>) {
    const now = setting();
    if (!now || saving()) return;
    // Vẽ lạc quan trước: một công tắc đợi hết vòng gọi mới nhảy thì cảm giác như nó kẹt.
    const next = { ...now, ...patch };
    setSetting(next);
    setSaving(true);
    setError(null);
    try {
      // Lõi siết `candidates` và `topN` về khoảng hợp lệ, nên câu trả lời có thể khác cái
      // vừa gửi. Vẽ theo nó thì ô số tự sửa trước mắt người dùng, thay vì hiện một giá
      // trị mà lõi đã lặng lẽ đổi.
      setSetting(
        await setRerank({
          enabled: next.enabled,
          backend: next.backend,
          model: next.model,
          candidates: next.candidates,
          topN: next.topN,
        }),
      );
    } catch (err) {
      setSetting(now);
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  /** Số người dùng gõ vào, hoặc `null` khi nó chưa phải một con số. */
  function asNumber(raw: string): number | null {
    const value = Number.parseInt(raw.trim(), 10);
    return Number.isFinite(value) ? value : null;
  }

  return (
    <section class="flex flex-col gap-3">
      <SectionHead
        title="Xếp hạng lại"
        icon="graph"
        desc="Chấm lại thứ tự những đoạn vừa tìm được."
        more="Một mô hình đọc cả câu hỏi lẫn từng đoạn cùng một lúc, nên nó xếp đúng hơn phép so vector — vốn nén hai bên tách rời nhau rồi mới so."
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" title="Không lưu được">
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={setting()}>
        {(value) => (
          <>
            <RowGroup>
              <Row
                label="Bật xếp hạng lại"
                icon="graph"
                desc="Tắt thì tìm nhanh hơn, thứ tự kém chính xác hơn."
                more="Tắt đi thì truy hồi vẫn chạy bằng cách hợp nhất từ khoá với vector; chỉ mất bước chấm lại ở cuối."
                control={() => (
                  <Toggle
                    label="Bật xếp hạng lại"
                    checked={value().enabled}
                    disabled={saving()}
                    busy={saving()}
                    onChange={(enabled) => void save({ enabled })}
                  />
                )}
              />

              <Show when={value().enabled}>
                <Row
                  label="Số đoạn chấm lại"
                  icon="list"
                  desc="Nút chỉnh độ trễ của tìm kiếm."
                  more="Lấy về nhiều thì thứ tự tốt hơn và chậm hơn. Trên máy không có GPU, mỗi đoạn tốn khoảng 0,4 giây."
                  control={() => (
                    <div class="w-[120px]">
                      <CommitField
                        label="Số đoạn chấm lại"
                        value={String(value().candidates)}
                        disabled={saving()}
                        onCommit={(raw) => {
                          const next = asNumber(raw);
                          if (next !== null) void save({ candidates: next });
                        }}
                      />
                    </div>
                  )}
                />

                <Row
                  label="Giữ lại"
                  icon="check"
                  desc="Bao nhiêu đoạn được đưa cho mô hình trả lời."
                  control={() => (
                    <div class="w-[120px]">
                      <CommitField
                        label="Số đoạn giữ lại"
                        value={String(value().topN)}
                        disabled={saving()}
                        onCommit={(raw) => {
                          const next = asNumber(raw);
                          if (next !== null) void save({ topN: next });
                        }}
                      />
                    </div>
                  )}
                />

                <Row
                  label="Chạy ở đâu"
                  icon="server"
                  desc="Trong máy, hoặc một máy chủ ngoài."
                  more="Trong máy: một tệp mô hình chạy cùng tiến trình đọc tài liệu, không có gì rời khỏi máy. Máy chủ ngoài: một endpoint /v1/rerank như TEI hoặc Infinity."
                  control={() => (
                    <Select
                      label="Nơi chạy mô hình xếp hạng lại"
                      value={value().backend}
                      disabled={saving()}
                      options={[
                        { id: "onnx", label: "Trong máy" },
                        { id: "http", label: "Máy chủ ngoài" },
                      ]}
                      onPick={(backend) =>
                        void save({ backend: backend as RerankSetting["backend"] })
                      }
                    />
                  )}
                />

                <Row
                  label={value().backend === "onnx" ? "Kho mô hình" : "Tên mô hình"}
                  icon="model"
                  desc={
                    value().backend === "onnx"
                      ? "Tên kho trên Hugging Face."
                      : "Mô hình mà máy chủ của bạn phục vụ."
                  }
                  more={
                    value().backend === "onnx"
                      ? "Tải về ở lần chạy đầu, khoảng hai gigabyte. Trong lúc chờ thì tìm kiếm vẫn chạy, chỉ là chưa có bước chấm lại."
                      : undefined
                  }
                  control={() => (
                    <div class="w-[280px] max-w-full">
                      <CommitField
                        label="Mô hình xếp hạng lại"
                        mono
                        value={value().model}
                        disabled={saving()}
                        onCommit={(model) => void save({ model })}
                      />
                    </div>
                  )}
                />
              </Show>
            </RowGroup>

            <Show when={value().reason}>
              {(reason) => (
                <Banner
                  tone={value().enabled ? "info" : "warn"}
                  icon={value().enabled ? "clock" : "warn"}
                  more="Đổi ở đây không nhúng lại thư viện — bước này chỉ sắp xếp lại những đoạn đã tìm được, nên câu hỏi kế tiếp đã theo cấu hình mới."
                >
                  {reason()}
                </Banner>
              )}
            </Show>
          </>
        )}
      </Show>
    </section>
  );
}
