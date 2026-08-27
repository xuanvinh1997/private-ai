import { Dialog } from "@kobalte/core/dialog";
import { Cloud, PlugZap, Plus, X } from "lucide-solid";
import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";
import { api } from "../api";
import type { ProviderDraft, ProviderKind, ProviderRecord } from "../types";

const kindLabels: Record<ProviderKind, string> = {
  ollama: "Ollama",
  openai: "OpenAI API",
};

const emptyDraft = (): ProviderDraft => ({
  name: "",
  kind: "openai",
  base_url: "",
  api_key: "",
});

const errorText = (cause: unknown, fallback: string) =>
  cause instanceof Error ? cause.message : fallback;

function ProviderDialog(props: {
  provider?: ProviderRecord;
  onSaved: () => void;
  trigger: string;
  triggerClass: string;
}) {
  const [open, setOpen] = createSignal(false);
  const [draft, setDraft] = createSignal<ProviderDraft>(emptyDraft());
  const [status, setStatus] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const editing = () => props.provider !== undefined;

  const reset = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (!nextOpen) return;
    setStatus("");
    setError("");
    const provider = props.provider;
    setDraft(
      provider
        ? { name: provider.name, kind: provider.kind, base_url: provider.base_url, api_key: "" }
        : emptyDraft(),
    );
  };

  const edit = <K extends keyof ProviderDraft>(key: K, value: ProviderDraft[K]) =>
    setDraft({ ...draft(), [key]: value });

  const probe = async () => {
    setBusy(true);
    setError("");
    setStatus("Đang kiểm tra kết nối…");
    try {
      const result = await api.probeProviderDraft({
        kind: draft().kind,
        base_url: draft().base_url.trim(),
        api_key: draft().api_key,
      });
      if (!result.reachable) {
        setStatus("");
        setError(result.detail ?? "Không kết nối được tới máy chủ này");
        return;
      }
      setStatus(
        `Kết nối thành công · ${result.model_count} mô hình${
          result.models.length ? ` (${result.models.slice(0, 3).join(", ")}…)` : ""
        }`,
      );
    } catch (cause) {
      setStatus("");
      setError(errorText(cause, "Không kiểm tra được kết nối"));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const value = draft();
    if (!value.name.trim() || !value.base_url.trim()) {
      setError("Cần nhập tên và địa chỉ máy chủ");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const provider = props.provider;
      if (provider) {
        await api.updateProvider(provider.id, {
          name: value.name.trim(),
          base_url: value.base_url.trim(),
          // An untouched key field means "keep the stored key" rather than "clear it".
          ...(value.api_key ? { api_key: value.api_key } : {}),
        });
      } else {
        await api.createProvider({
          ...value,
          name: value.name.trim(),
          base_url: value.base_url.trim(),
        });
      }
      props.onSaved();
      setOpen(false);
    } catch (cause) {
      setError(errorText(cause, "Không lưu được nhà cung cấp"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open()} onOpenChange={reset}>
      <Dialog.Trigger class={props.triggerClass}>
        <Show when={!editing()}><Plus size={18} /></Show>
        {props.trigger}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content class="dialog-content">
            <div class="dialog-mark"><Cloud size={22} /></div>
            <Dialog.Title>{editing() ? props.provider!.name : "Thêm nhà cung cấp AI"}</Dialog.Title>
            <Dialog.Description>
              <Show
                when={props.provider?.builtin}
                fallback="Kết nối tới một máy chủ nói giao thức OpenAI API, ví dụ vLLM, LM Studio, LiteLLM hoặc OpenAI. Khóa API chỉ được lưu trên máy này."
              >
                Đổi địa chỉ Ollama mà ứng dụng gọi tới, ví dụ khi Ollama chạy trong WSL2 hoặc
                trên một máy khác trong mạng nội bộ.
              </Show>
            </Dialog.Description>

            <label class="field-label" for="provider-name">Tên hiển thị</label>
            <input
              id="provider-name"
              class="text-input"
              autocomplete="off"
              placeholder="Máy chủ nội bộ"
              value={draft().name}
              onInput={(event) => edit("name", event.currentTarget.value)}
            />

            <Show when={!editing()}>
              <label class="field-label field-spaced" for="provider-kind">Giao thức</label>
              <select
                id="provider-kind"
                class="text-input"
                value={draft().kind}
                onChange={(event) => edit("kind", event.currentTarget.value as ProviderKind)}
              >
                <option value="openai">{kindLabels.openai}</option>
                <option value="ollama">{kindLabels.ollama}</option>
              </select>
            </Show>

            <label class="field-label field-spaced" for="provider-url">Địa chỉ máy chủ</label>
            <input
              id="provider-url"
              class="text-input"
              autocomplete="off"
              placeholder="https://api.openai.com/v1"
              value={draft().base_url}
              onInput={(event) => edit("base_url", event.currentTarget.value)}
            />

            <Show when={draft().kind === "openai"}>
              <label class="field-label field-spaced" for="provider-key">Khóa API</label>
              <input
                id="provider-key"
                class="text-input"
                type="password"
                autocomplete="off"
                placeholder={props.provider?.has_api_key ? "Giữ nguyên khóa đã lưu" : "sk-…"}
                value={draft().api_key}
                onInput={(event) => edit("api_key", event.currentTarget.value)}
              />
            </Show>

            <Show when={status()}><p class="field-status">{status()}</p></Show>
            <Show when={error()}><p class="field-error">{error()}</p></Show>

            <div class="dialog-actions dialog-actions-split">
              <button class="button button-secondary" type="button" disabled={busy() || !draft().base_url.trim()} onClick={probe}>
                <PlugZap size={18} /> Kiểm tra
              </button>
              <div>
                <Dialog.CloseButton class="button button-secondary">Hủy</Dialog.CloseButton>
                <button class="button button-primary" type="button" disabled={busy()} onClick={save}>
                  {editing() ? "Lưu" : "Thêm"}
                </button>
              </div>
            </div>
            <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng hộp thoại"><X size={20} /></Dialog.CloseButton>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}

function ProviderRow(props: { provider: ProviderRecord; onChanged: () => void }) {
  const [busy, setBusy] = createSignal(false);
  const [status, setStatus] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal(false);

  const run = async (action: () => Promise<void>, fallback: string) => {
    setBusy(true);
    try {
      await action();
      setStatus("");
    } catch (cause) {
      setStatus(errorText(cause, fallback));
    } finally {
      setBusy(false);
    }
  };

  const activate = () =>
    run(async () => {
      await api.activateProvider(props.provider.id);
      props.onChanged();
    }, "Không chuyển được nhà cung cấp");

  const probe = async () => {
    setBusy(true);
    setStatus("Đang kiểm tra kết nối…");
    try {
      const result = await api.probeProvider(props.provider.id);
      setStatus(
        result.reachable
          ? `Kết nối tốt · ${result.model_count} mô hình`
          : (result.detail ?? "Không kết nối được"),
      );
    } catch (cause) {
      setStatus(errorText(cause, "Không kiểm tra được kết nối"));
    } finally {
      setBusy(false);
    }
  };

  const remove = () => {
    if (!confirmDelete()) {
      setConfirmDelete(true);
      return;
    }
    return run(async () => {
      await api.deleteProvider(props.provider.id);
      props.onChanged();
    }, "Không xóa được nhà cung cấp");
  };

  return (
    <article class="provider-row" classList={{ active: props.provider.active }}>
      <div class="provider-identity">
        <strong>{props.provider.name}</strong>
        <span>{kindLabels[props.provider.kind]} · {props.provider.base_url}</span>
        <Show when={props.provider.has_api_key}><small>Đã lưu khóa API</small></Show>
        <Show when={props.provider.builtin}><small>Ollama trên máy này</small></Show>
        <Show when={status()}><small>{status()}</small></Show>
      </div>
      <div class="provider-state">
        <Show when={props.provider.active} fallback={<span class="provider-idle">Chưa dùng</span>}>
          <span class="provider-active">Đang dùng</span>
        </Show>
      </div>
      <div class="provider-actions">
        <Show when={!props.provider.active}>
          <button disabled={busy()} onClick={activate}>Dùng</button>
        </Show>
        <button disabled={busy()} onClick={probe}>Kiểm tra</button>
        <ProviderDialog
          provider={props.provider}
          trigger="Sửa"
          triggerClass="provider-action-trigger"
          onSaved={props.onChanged}
        />
        <button classList={{ danger: confirmDelete() }} disabled={busy()} onClick={remove}>
          {confirmDelete() ? "Xác nhận xóa" : "Xóa"}
        </button>
      </div>
    </article>
  );
}

export function ProviderSettings(props: { onChanged: () => void }) {
  const [providers, { refetch }] = createResource(api.providers);

  const reload = () => {
    void refetch();
    props.onChanged();
  };

  return (
    <section class="provider-settings">
      <div class="page-heading page-heading-row">
        <div>
          <span>Nguồn suy luận</span>
          <h2>Nhà cung cấp AI</h2>
          <p>
            Chọn nơi chạy mô hình: Ollama trên máy hoặc bất kỳ máy chủ nào theo chuẩn OpenAI API.
            Trò chuyện, embedding và trích xuất tri thức đều dùng nhà cung cấp đang bật.
          </p>
        </div>
        <ProviderDialog
          trigger="Thêm nhà cung cấp"
          triggerClass="button button-primary"
          onSaved={reload}
        />
      </div>
      <div class="provider-list">
        <Switch fallback={
          <For each={providers()}>
            {(provider) => <ProviderRow provider={provider} onChanged={reload} />}
          </For>
        }>
          <Match when={providers.loading}>
            <div class="loading-row"><i />Đang đọc cấu hình nhà cung cấp…</div>
          </Match>
          <Match when={providers()?.length === 0}>
            <div class="empty-models">
              <strong>Chưa có nhà cung cấp nào</strong>
              <span>Thêm một máy chủ để trò chuyện, tạo embedding và trích xuất tri thức.</span>
            </div>
          </Match>
          <Match when={providers.error}>
            <div class="empty-models">
              <strong>Không đọc được danh sách nhà cung cấp</strong>
              <span>{errorText(providers.error, "Máy chủ API không phản hồi")}</span>
              <span>Nếu API đang chạy từ trước bản cập nhật này, hãy khởi động lại nó.</span>
            </div>
          </Match>
        </Switch>
      </div>
    </section>
  );
}
