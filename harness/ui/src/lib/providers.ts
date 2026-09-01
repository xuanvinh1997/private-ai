import { invoke } from "@tauri-apps/api/core";
import { inTauri, listModels } from "./agent";
import { isDemo } from "./demo";
import {
  demoActiveModels,
  demoProbeProvider,
  demoProviderPresets,
  demoProviders,
  demoRemoveProvider,
  demoSaveProvider,
  demoSetActiveProvider,
  demoSetProviderModel,
} from "./fixtures/providers";
import type { ModelChoice, Provider, ProviderInput, ProviderPreset, ProviderProbe } from "./protocol";

/**
 * Bảy lệnh provider, chia hai nhóm theo cách xử lý lỗi — cùng ranh giới với `projects.ts`:
 * "người dùng có đang đứng chờ một thứ hiện lên không".
 *
 *   - `listProviders`, `providerPresets`, `loadModels` chạy lúc mở màn hình. Chúng nuốt
 *     lỗi và trả rỗng: một hộp lỗi ở đó chặn mất lối vào trang cài đặt, mà trang cài đặt
 *     lại đúng là chỗ người dùng đi tới để *sửa* cái đang hỏng.
 *   - `saveProvider`, `removeProvider`, `setActiveProvider`, `setProviderModel`,
 *     `probeProvider` **ném ra ngoài**. Cả năm đều đi sau một cú bấm, và im lặng ở đó
 *     không phân biệt được với "đang chậm" — người dùng sẽ bấm lần hai.
 *
 * Chế độ `?demo=1` rẽ nhánh ở đây chứ không ở component: màn hình không cần biết dữ liệu
 * của nó đến từ lõi hay từ một mảng trong bộ nhớ, và mỗi chỗ rẽ nhánh trong component là
 * một nhánh chỉ chạy trong demo, tức là một nhánh không ai kiểm.
 */

export async function listProviders(): Promise<Provider[]> {
  if (isDemo()) return demoProviders();
  if (!inTauri()) return [];
  try {
    return await invoke<Provider[]>("list_providers");
  } catch (err) {
    console.error("không đọc được danh sách provider", err);
    return [];
  }
}

/** Danh mục dựng sẵn. Rỗng chỉ có nghĩa "không gợi ý được gì", không phải hỏng. */
export async function providerPresets(): Promise<ProviderPreset[]> {
  if (isDemo()) return demoProviderPresets();
  if (!inTauri()) return [];
  try {
    return await invoke<ProviderPreset[]>("provider_presets");
  } catch (err) {
    console.error("không đọc được danh mục provider", err);
    return [];
  }
}

/**
 * Lưu một provider. `input.id` rỗng (`null`) là thêm mới.
 *
 * `input.apiKey === null` nghĩa là **giữ nguyên khoá đã lưu**; chuỗi rỗng mới là xoá.
 * Quy ước đó do hợp đồng đặt ra, và biểu mẫu phải nói lại nó bằng tiếng người trên màn
 * hình — một người đổi tên provider rồi mất khoá sẽ không bao giờ đoán ra vì sao.
 */
export function saveProvider(input: ProviderInput): Promise<Provider> {
  if (isDemo()) return Promise.resolve(demoSaveProvider(input));
  return invoke<Provider>("save_provider", { input });
}

export function removeProvider(id: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoRemoveProvider(id));
  return invoke("remove_provider", { id });
}

/** Chọn provider sẽ chạy lượt tiếp theo. Chỉ một cái hoạt động tại một thời điểm. */
export function setActiveProvider(id: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetActiveProvider(id));
  return invoke("set_active_provider", { id });
}

export function setProviderModel(id: string, model: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetProviderModel(id, model));
  return invoke("set_provider_model", { id, model });
}

/**
 * Thử một cấu hình **chưa lưu**.
 *
 * Đây là lý do lệnh nhận cả `ProviderInput` chứ không nhận một `id`: giá trị đáng thử
 * nhất là giá trị người dùng vừa gõ vào và chưa dám lưu. Với provider đã có khoá thì để
 * `apiKey: null` và lõi tự lấy khoá cũ ra dùng.
 *
 * **`ProviderProbe.models[].tools` ở đây không có thẩm quyền.** Một lần thử cố ý không
 * trả tiền để hỏi năng lực gọi tool của từng mô hình, nên lõi trả `false` cho tất cả.
 * Giao diện phải đọc cờ đó từ `activeModels()`; hiện cảnh báo "không gọi được tool" từ
 * kết quả thử là dán nhãn sai lên toàn bộ danh sách, và một cảnh báo luôn bật là một
 * cảnh báo không ai đọc nữa.
 */
export function probeProvider(input: ProviderInput): Promise<ProviderProbe> {
  if (isDemo()) return Promise.resolve(demoProbeProvider(input));
  return invoke<ProviderProbe>("probe_provider", { input });
}

/**
 * Mô hình của provider **đang hoạt động** — nguồn có thẩm quyền cho cờ `tools`.
 *
 * Đi qua `list_models` chứ không qua `probe_provider` vì đúng một lý do: chỉ ở đây lõi
 * mới thật sự hỏi từng mô hình xem nó gọi được tool không. Bộ chọn mô hình treo cả một
 * cảnh báo lên cờ đó, nên nó phải lấy từ chỗ cờ đó đúng.
 *
 * Nuốt lỗi (`list_models` của `agent.ts` đã nuốt sẵn): nó chạy lúc mở màn hình, và danh
 * sách rỗng nghĩa là **máy chủ không trả lời được**, không phải "không có mô hình nào".
 */
export async function activeModels(): Promise<ModelChoice[]> {
  if (isDemo()) return demoActiveModels();
  return await listModels();
}

/** `ProviderInput` để thử/đọc mô hình của một provider đã lưu, giữ nguyên khoá của nó. */
export function inputOf(provider: Provider): ProviderInput {
  return {
    id: provider.id,
    name: provider.name,
    kind: provider.kind,
    baseUrl: provider.baseUrl,
    apiKey: null,
    enabled: provider.enabled,
    model: provider.model,
  };
}
