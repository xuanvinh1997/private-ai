import { invoke } from "@tauri-apps/api/core";
import { inTauri, listModels } from "./agent";
import { isDemo } from "./demo";
import { S, t, type Msg } from "./i18n";
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
  demoProbeVision,
  demoSetEmbedding,
  demoSetOcrEnabled,
  demoSetOcrImages,
  demoSetProviderModel,
  demoSetVision,
  demoVisionSetting,
} from "./fixtures/providers";
import type {
  ChunkSetting,
  EmbeddingProbe,
  EmbeddingSetting,
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderPreset,
  ProviderProbe,
  RerankSetting,
  VisionProbe,
  VisionSetting,
} from "./protocol";

/** Provider commands, split like `projects.ts`: screen-load calls swallow errors and return defaults, click-driven
 * ones throw. `?demo=1` branches here, not in components, so no component carries a demo-only path. */

export async function listProviders(): Promise<Provider[]> {
  if (isDemo()) return demoProviders();
  if (!inTauri()) return [];
  try {
    return await invoke<Provider[]>("list_providers");
  } catch (err) {
    console.error("failed to list providers", err);
    return [];
  }
}

/** Built-in presets. Empty only means "nothing to suggest", not a failure. */
export async function providerPresets(): Promise<ProviderPreset[]> {
  if (isDemo()) return demoProviderPresets();
  if (!inTauri()) return [];
  try {
    return await invoke<ProviderPreset[]>("provider_presets");
  } catch (err) {
    console.error("failed to read provider presets", err);
    return [];
  }
}

/** Save a provider; a `null` `input.id` adds one, and `apiKey === null` keeps the stored key while "" clears it. */
export function saveProvider(input: ProviderInput): Promise<Provider> {
  if (isDemo()) return Promise.resolve(demoSaveProvider(input));
  return invoke<Provider>("save_provider", { input });
}

export function removeProvider(id: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoRemoveProvider(id));
  return invoke("remove_provider", { id });
}

/** Pick the provider for the next *chat* turn; the embedding role is untouched, since moving it is a privacy change. */
export function setActiveProvider(id: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetActiveProvider(id));
  return invoke("set_active_provider", { id });
}

export function setProviderModel(id: string, model: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetProviderModel(id, model));
  return invoke("set_provider_model", { id, model });
}

/** The model the core will use for the next chat turn, including a preset default that has not been written yet. */
export async function activeChatModel(): Promise<string> {
  if (isDemo()) {
    return demoProviders().find((provider) => provider.activeChat)?.model ?? "";
  }
  if (!inTauri()) return "";
  try {
    return await invoke<string>("active_chat_model");
  } catch (err) {
    console.error("failed to read active chat model", err);
    return "";
  }
}

/** Persist a model on the active chat provider and apply it to the shared driver. */
export async function setActiveChatModel(model: string): Promise<string> {
  if (isDemo()) {
    const active = demoProviders().find((provider) => provider.activeChat);
    if (active === undefined) throw new Error("Chưa cấu hình nhà cung cấp AI nào");
    demoSetProviderModel(active.id, model);
    return model;
  }
  return invoke<string>("set_active_chat_model", { model });
}

/** Probe an *unsaved* config, which is why it takes a `ProviderInput`. `models[].tools` here is NOT authoritative
 * (read it from `activeModels()`); `models[].embedding` is usable, since a wrong guess only reorders a picker. */
export function probeProvider(input: ProviderInput): Promise<ProviderProbe> {
  if (isDemo()) return Promise.resolve(demoProbeProvider(input));
  return invoke<ProviderProbe>("probe_provider", { input });
}

/** Models of the *active* provider, the authoritative source for the `tools` flag; empty means the server did not answer. */
export async function activeModels(): Promise<ModelChoice[]> {
  if (isDemo()) return demoActiveModels();
  return await listModels();
}

/** Models of *any* provider with their `embedding` flag; empty means the server was unreachable, so keep manual entry. */
export async function providerModels(providerId: string): Promise<ModelChoice[]> {
  if (isDemo()) return demoProviderModels(providerId);
  if (!inTauri()) return [];
  try {
    return await invoke<ModelChoice[]>("provider_models", { providerId });
  } catch (err) {
    console.error("failed to read provider models", err);
    return [];
  }
}

/** The *effective* embedding config, read from the core rather than inferred, because only it knows `reason`. */
/** Safe default when the core is unreachable: retrieval continues without optional reranking. */
const RERANK_MAC_DINH: RerankSetting = {
  enabled: false,
  backend: "onnx",
  model: "BAAI/bge-reranker-v2-m3",
  candidates: 30,
  topN: 8,
  reason: null,
};

/** Safe default when the core is unreachable: the same numbers `pai-rag` falls back to. */
const CHUNK_MAC_DINH: ChunkSetting = { size: 1400, overlap: 180, reason: null };

/** How documents are cut. Read from the core, not assumed, since an absent key means the service's own default. */
export async function chunkSetting(): Promise<ChunkSetting> {
  if (!inTauri()) return CHUNK_MAC_DINH;
  try {
    return await invoke<ChunkSetting>("chunk_setting");
  } catch (err) {
    console.error("failed to read chunk setting", err);
    return CHUNK_MAC_DINH;
  }
}

/** Persist both numbers; the core clamps them and returns what it stored, so the form redraws from the answer. */
export function setChunk(size: number, overlap: number): Promise<ChunkSetting> {
  return invoke<ChunkSetting>("set_chunk", { size, overlap });
}

export async function rerankSetting(): Promise<RerankSetting> {
  if (!inTauri()) return RERANK_MAC_DINH;
  try {
    return await invoke<RerankSetting>("rerank_setting");
  } catch (err) {
    console.error("failed to read rerank setting", err);
    return RERANK_MAC_DINH;
  }
}

/** Persist the rerank setting; unlike `setEmbedding` it re-embeds nothing, so the next question already uses it. */
export function setRerank(next: Omit<RerankSetting, "reason">): Promise<RerankSetting> {
  return invoke<RerankSetting>("set_rerank", {
    enabled: next.enabled,
    candidates: next.candidates,
    topN: next.topN,
  });
}

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
    console.error("failed to read embedding setting", err);
    return none;
  }
}

/** Assign the embedding role; changing the model makes the core drop every vector and re-embed, so confirm first. */
export function setEmbedding(providerId: string, model: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetEmbedding(providerId, model));
  return invoke("set_embedding", { providerId, model });
}

/** Actually embed one sentence and measure the vector; a reachable model list proves nothing about embedding. */
export function probeEmbedding(providerId: string, model: string): Promise<EmbeddingProbe> {
  if (isDemo()) return Promise.resolve(demoProbeEmbedding(providerId, model));
  return invoke<EmbeddingProbe>("probe_embedding", { providerId, model });
}

/** The *effective* vision config. Unset is a normal state: documents with a text layer still index, images wait. */
export async function visionSetting(): Promise<VisionSetting> {
  const none: VisionSetting = {
    providerId: null,
    providerName: null,
    model: null,
    onDevice: false,
    reason: null,
    ocrEnabled: true,
    ocrImages: false,
  };
  if (isDemo()) return demoVisionSetting();
  if (!inTauri()) return none;
  try {
    return await invoke<VisionSetting>("vision_setting");
  } catch (err) {
    console.error("failed to read vision setting", err);
    return none;
  }
}

/** Assign the vision role. Unlike `setEmbedding` nothing is re-embedded: it only changes who reads the *next* scan. */
export function setVision(providerId: string, model: string): Promise<VisionSetting> {
  if (isDemo()) return Promise.resolve(demoSetVision(providerId, model));
  return invoke<VisionSetting>("set_vision", { providerId, model });
}

/** The OCR switch, the same setting the library screen shows: off skips images instead of reporting them broken. */
export async function setOcr(enabled: boolean): Promise<VisionSetting> {
  if (isDemo()) return demoSetOcrEnabled(enabled);
  await invoke("set_ocr_enabled", { enabled });
  return visionSetting();
}

/** The optional half: reading the pictures inside pages that already have text. Off is the sane default —
 * a report full of photos would otherwise cost one model call per picture for nothing. */
export async function setOcrImages(enabled: boolean): Promise<VisionSetting> {
  if (isDemo()) return demoSetOcrImages(enabled);
  return invoke<VisionSetting>("set_ocr_images", { enabled });
}

/** Actually read a bundled test image; a model list never says which models can see. */
export function probeVision(providerId: string, model: string): Promise<VisionProbe> {
  if (isDemo()) return Promise.resolve(demoProbeVision(providerId, model));
  return invoke<VisionProbe>("probe_vision", { providerId, model });
}

/** Suggested vision model per provider kind: an editable prefill, not a closed choice. */
export function suggestedVisionModel(kind: Provider["kind"]): string {
  switch (kind) {
    case "ollama":
      return "qwen2.5vl:7b";
    case "lmstudio":
      return "qwen2.5-vl-7b-instruct";
    default:
      return "gpt-4o-mini";
  }
}

/** A `ProviderInput` for probing or listing a saved provider's models, keeping its stored key. */
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
    visionModel: provider.visionModel,
  };
}

/** Suggested embedding model per provider kind: an editable prefill, not a closed choice. */
export function suggestedEmbeddingModel(kind: Provider["kind"]): string {
  switch (kind) {
    case "ollama":
      return "nomic-embed-text";
    // LM Studio has no `text-embedding-3-small` (that is OpenAI's); suggesting a missing name only yields a 404.
    case "lmstudio":
      return "text-embedding-nomic-embed-text-v1.5";
    default:
      return "text-embedding-3-small";
  }
}

/** Translated preset hint, looked up by the stable `id` so the protocol need not change and unknown ids fall back. */
export function presetHint(preset: ProviderPreset): string {
  const table: Record<string, Msg> = S.providers.presetHint;
  const msg = table[preset.id];
  return msg === undefined ? preset.hint : t(msg);
}
