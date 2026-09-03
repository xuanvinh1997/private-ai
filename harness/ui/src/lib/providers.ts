import { invoke } from "@tauri-apps/api/core";
import { inTauri, listModels } from "./agent";
import { isDemo } from "./demo";
import {
  demoActiveModels,
  demoEmbeddingSetting,
  demoProbeEmbedding,
  demoProbeProvider,
  demoProviderModels,
  demoProviderPresets,
  demoProviders,
  demoRemoveProvider,
  demoSaveProvider,
  demoSetActiveProvider,
  demoSetEmbedding,
  demoSetProviderModel,
} from "./fixtures/providers";
import type {
  EmbeddingProbe,
  EmbeddingSetting,
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderPreset,
  ProviderProbe,
} from "./protocol";

/**
 * Mười một lệnh provider, chia hai nhóm theo cách xử lý lỗi — cùng ranh giới với
 * `projects.ts`: "người dùng có đang đứng chờ một thứ hiện lên không".
 *
 *   - `listProviders`, `providerPresets`, `loadModels`, `providerModels`,
 *     `embeddingSetting` chạy lúc mở màn hình. Chúng nuốt lỗi và trả mặc định: một hộp lỗi ở đó chặn mất lối vào trang
 *     cài đặt, mà trang cài đặt lại đúng là chỗ người dùng đi tới để *sửa* cái đang hỏng.
 *   - `saveProvider`, `removeProvider`, `setActiveProvider`, `setProviderModel`,
 *     `setEmbedding`, `probeProvider`, `probeEmbedding` **ném ra ngoài**. Cả bảy đều đi
 *     sau một cú bấm, và im lặng ở đó không phân biệt được với "đang chậm" — người dùng
 *     sẽ bấm lần hai.
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

/**
 * Chọn provider sẽ chạy lượt **hội thoại** tiếp theo. Chỉ một cái giữ vai này.
 *
 * Không đụng tới vai nhúng: tài liệu vẫn đi tới provider đã chọn ở màn hình mô hình
 * nhúng. Đổi mô hình trò chuyện mà kéo theo cả chỗ tài liệu được gửi tới là một thay đổi
 * về quyền riêng tư xảy ra sau lưng người dùng.
 */
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
 * **Hai cờ, hai mức chắc chắn — đừng dùng lẫn.**
 *
 *   - `models[].tools` ở đây **không có thẩm quyền**: một lần thử cố ý không trả tiền để
 *     hỏi năng lực gọi tool của từng mô hình, nên lõi trả `false` cho gần hết. Giao diện
 *     phải đọc cờ đó từ `activeModels()`; hiện cảnh báo "không gọi được tool" từ kết quả
 *     thử là dán nhãn sai lên toàn bộ danh sách, và một cảnh báo luôn bật là một cảnh báo
 *     không ai đọc nữa.
 *   - `models[].embedding` thì **dùng được ngay**. Nó cũng chỉ là đoán theo tên ở Ollama
 *     và OpenAI-compatible (ở LM Studio thì có thẩm quyền), nhưng hậu quả của một lần
 *     đoán trượt khác hẳn: nó chỉ xếp một mô hình xuống dưới trong ô chọn mô hình nhúng,
 *     chứ không dán nhãn hỏng lên nó — và không có nó thì người dùng phải tự nhớ tên mô
 *     hình nhúng của máy chủ mình.
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

/**
 * Kho mô hình của **một provider bất kỳ**, kèm cờ `embedding` của từng cái.
 *
 * Khác `activeModels()` ở đúng chỗ màn hình mô hình nhúng cần: `activeModels()` chỉ biết
 * provider đang giữ vai *hội thoại*, mà vai nhúng thường nằm trên một máy chủ khác —
 * nhúng tại chỗ, trò chuyện từ xa là cấu hình mà việc tách hai vai tồn tại để phục vụ.
 *
 * Nuốt lỗi và trả rỗng: nó chạy ngay khi người dùng vừa chọn provider, và rỗng ở đây có
 * nghĩa **không hỏi được máy chủ** — một trạng thái bình thường (máy chủ chưa bật, hoặc
 * provider từ xa không liệt kê). Nơi gọi phải giữ lối nhập tay cho đúng trường hợp đó.
 */
export async function providerModels(providerId: string): Promise<ModelChoice[]> {
  if (isDemo()) return demoProviderModels(providerId);
  if (!inTauri()) return [];
  try {
    return await invoke<ModelChoice[]>("provider_models", { providerId });
  } catch (err) {
    console.error("không đọc được kho mô hình của provider", err);
    return [];
  }
}

/**
 * Cấu hình nhúng **đang có hiệu lực**.
 *
 * Đọc riêng chứ không suy ra từ `listProviders()`: chỉ lõi mới biết một cấu hình có tên
 * đầy đủ vẫn không dùng được (provider bị tắt, mô hình chưa chọn), và nó nói ra điều đó
 * trong `reason`. Suy lại ở phía này là dựng một bản luật thứ hai sẽ lệch sau lần sửa lõi
 * đầu tiên.
 *
 * Nuốt lỗi: chạy lúc mở màn hình, và "chưa cấu hình" là một trạng thái hợp lệ chứ không
 * phải một hỏng hóc — thư viện tài liệu khi đó vẫn tìm được bằng từ khoá.
 */
export async function embeddingSetting(): Promise<EmbeddingSetting> {
  const none: EmbeddingSetting = {
    providerId: null,
    providerName: null,
    model: null,
    onDevice: false,
    reason: null,
  };
  if (isDemo()) return demoEmbeddingSetting();
  if (!inTauri()) return none;
  try {
    return await invoke<EmbeddingSetting>("embedding_setting");
  } catch (err) {
    console.error("không đọc được cấu hình nhúng", err);
    return none;
  }
}

/**
 * Giao vai nhúng cho một provider và chốt mô hình nhúng của nó.
 *
 * Lệnh này **làm lõi bỏ toàn bộ vector cũ và nhúng lại cả thư viện** khi mô hình đổi:
 * vector của hai mô hình nằm ở hai không gian khác nhau, và so sánh chúng cho ra một con
 * số vô nghĩa trông y hệt một con số có nghĩa. Nơi gọi phải hỏi xác nhận trước.
 */
export function setEmbedding(providerId: string, model: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetEmbedding(providerId, model));
  return invoke("set_embedding", { providerId, model });
}

/**
 * Thử nhúng **thật một câu** và đo số chiều của vector trả về.
 *
 * Khác hẳn `probeProvider`, và khác ở đúng chỗ quan trọng: `/api/tags` của Ollama trả về
 * mọi mô hình và không có gì trong đó nói cái nào nhúng được, nên một danh sách "nối
 * được" không chứng minh gì cả. Chỉ khi một câu đi qua và một vector quay về thì mới biết
 * chắc — và số chiều là bằng chứng của việc đó.
 */
export function probeEmbedding(providerId: string, model: string): Promise<EmbeddingProbe> {
  if (isDemo()) return Promise.resolve(demoProbeEmbedding(providerId, model));
  return invoke<EmbeddingProbe>("probe_embedding", { providerId, model });
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
    embeddingModel: provider.embeddingModel,
  };
}

/**
 * Mô hình nhúng gợi ý theo loại provider.
 *
 * Là **giá trị điền sẵn sửa được**, không phải một lựa chọn đã chốt: người dùng có thể đã
 * kéo về `mxbai-embed-large` hay `bge-m3`, và một ô chỉ cho chọn trong hai cái tên dưới
 * đây là một ô nói rằng máy của họ chỉ có hai mô hình.
 */
export function suggestedEmbeddingModel(kind: Provider["kind"]): string {
  switch (kind) {
    case "ollama":
      return "nomic-embed-text";
    // Kho của LM Studio không có `text-embedding-3-small` — đó là mô hình của OpenAI. Gợi
    // ý một cái tên không tồn tại tệ hơn không gợi ý gì: người dùng dán nó vào rồi đọc
    // một lỗi 404 chẳng nói được vì sao.
    case "lmstudio":
      return "text-embedding-nomic-embed-text-v1.5";
    default:
      return "text-embedding-3-small";
  }
}
