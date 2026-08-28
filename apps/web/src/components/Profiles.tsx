import { Dialog } from "@kobalte/core/dialog";
import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { Check, ChevronsUpDown, Pencil, Plus, Settings2, Trash2, UserPlus, X } from "lucide-solid";
import { For, Show, createSignal } from "solid-js";
import { api } from "../api";
import type { ProfileRecord } from "../types";

/** "Phạm Xuân Vinh" → "PV", so the avatar follows whatever name the person chose. */
export function initialsOf(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return "?";
  const letters =
    parts.length === 1 ? parts[0].slice(0, 2) : `${parts[0][0]}${parts[parts.length - 1][0]}`;
  return letters.toUpperCase();
}

type NameMode = "onboarding" | "rename" | "create";

const COPY: Record<NameMode, { title: string; description: string; action: string }> = {
  onboarding: {
    title: "Chào bạn, mình gọi bạn là gì?",
    description:
      "Tên này chỉ được lưu trên máy của bạn và dùng để xưng hô trong ứng dụng. Bạn đổi lại lúc nào cũng được.",
    action: "Bắt đầu",
  },
  rename: {
    title: "Đổi tên hiển thị",
    description: "Tên mới áp dụng ngay cho lời chào và ô trò chuyện.",
    action: "Lưu",
  },
  create: {
    title: "Thêm hồ sơ",
    description:
      "Hồ sơ mới có bộ nhớ riêng và sẽ được dùng ngay. Tài liệu cùng không gian làm việc vẫn dùng chung trên máy này.",
    action: "Tạo và chuyển sang",
  },
};

export function ProfileNameDialog(props: {
  open: boolean;
  mode: NameMode;
  profile?: ProfileRecord;
  onClose: () => void;
  onDone: (profile: ProfileRecord) => void;
}) {
  const [name, setName] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal("");

  const prepare = (open: boolean) => {
    if (open) {
      setName(props.mode === "rename" ? (props.profile?.display_name ?? "") : "");
      setError("");
      return;
    }
    // Onboarding has no answer to fall back on, so it stays until a name is given.
    if (props.mode !== "onboarding") props.onClose();
  };

  const save = async () => {
    const value = name().trim();
    if (!value) {
      setError("Hãy nhập tên bạn muốn hiển thị.");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const saved =
        props.mode === "create"
          ? await api.createProfile(value)
          : await api.renameProfile(props.profile!.id, value);
      props.onDone(saved);
      props.onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không lưu được tên");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={props.open} onOpenChange={prepare} modal>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content
            class="dialog-content"
            onEscapeKeyDown={(event: Event) => {
              if (props.mode === "onboarding") event.preventDefault();
            }}
          >
            <div class="dialog-mark"><UserPlus size={22} /></div>
            <Dialog.Title>{COPY[props.mode].title}</Dialog.Title>
            <Dialog.Description>{COPY[props.mode].description}</Dialog.Description>
            <label class="field-label" for="profile-name">Tên hiển thị</label>
            <input
              id="profile-name"
              class="text-input"
              autofocus
              maxLength={60}
              placeholder="Ví dụ: Vinh"
              value={name()}
              onInput={(event) => setName(event.currentTarget.value)}
              onKeyDown={(event) => { if (event.key === "Enter") void save(); }}
            />
            <Show when={error()}><p class="field-error">{error()}</p></Show>
            <div class="dialog-actions">
              <Show when={props.mode !== "onboarding"}>
                <button class="button button-secondary" type="button" onClick={props.onClose}>Hủy</button>
              </Show>
              <Show when={props.mode === "onboarding"}>
                <button class="button button-secondary" type="button" onClick={props.onClose}>Để sau</button>
              </Show>
              <button class="button button-primary" type="button" disabled={saving()} onClick={save}>
                {saving() ? "Đang lưu…" : COPY[props.mode].action}
              </button>
            </div>
            <Show when={props.mode !== "onboarding"}>
              <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng"><X size={20} /></Dialog.CloseButton>
            </Show>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}

export function ProfileSwitcher(props: {
  profiles: ProfileRecord[];
  active?: ProfileRecord;
  online: boolean;
  onChanged: () => void;
  onOpenSettings: () => void;
}) {
  const [dialog, setDialog] = createSignal<NameMode>();
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  const [error, setError] = createSignal("");

  const name = () => props.active?.display_name?.trim() || "Bạn";

  const switchTo = async (id: string) => {
    if (id === props.active?.id) return;
    setError("");
    try {
      await api.activateProfile(id);
      props.onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không chuyển được hồ sơ");
    }
  };

  const remove = async () => {
    if (!props.active) return;
    if (!confirmDelete()) {
      setConfirmDelete(true);
      return;
    }
    setError("");
    try {
      await api.deleteProfile(props.active.id);
      setConfirmDelete(false);
      props.onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không xóa được hồ sơ");
    }
  };

  return (
    <>
      <DropdownMenu placement="top-start" gutter={8} onOpenChange={() => setConfirmDelete(false)}>
        <DropdownMenu.Trigger class="account-button" aria-label={`Hồ sơ ${name()}`}>
          <span class="account-avatar" aria-hidden="true">{initialsOf(name())}</span>
          <span class="account-copy">
            <strong>{name()}</strong>
            <small>
              <span classList={{ "status-pip": true, "status-online": props.online, "status-offline": !props.online }} aria-hidden="true" />
              Trên thiết bị
            </small>
          </span>
          <ChevronsUpDown size={16} aria-hidden="true" />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class="menu-content">
            <div class="menu-label">Hồ sơ trên máy này</div>
            <For each={props.profiles}>{(profile) => (
              <DropdownMenu.Item
                class="menu-item"
                onSelect={() => void switchTo(profile.id)}
              >
                <span class="menu-avatar" aria-hidden="true">{initialsOf(profile.display_name || "?")}</span>
                <span class="menu-copy">
                  <strong>{profile.display_name || "Chưa đặt tên"}</strong>
                  <small>{profile.memory_count} mục bộ nhớ</small>
                </span>
                <Show when={profile.active}><Check size={16} /></Show>
              </DropdownMenu.Item>
            )}</For>
            <DropdownMenu.Separator class="menu-separator" />
            <DropdownMenu.Item class="menu-item menu-item-plain" onSelect={() => setDialog("create")}>
              <Plus size={17} /> Thêm hồ sơ
            </DropdownMenu.Item>
            <DropdownMenu.Item class="menu-item menu-item-plain" onSelect={() => setDialog("rename")}>
              <Pencil size={17} /> Đổi tên hiển thị
            </DropdownMenu.Item>
            <DropdownMenu.Item class="menu-item menu-item-plain" onSelect={props.onOpenSettings}>
              <Settings2 size={17} /> Cài đặt
            </DropdownMenu.Item>
            <Show when={props.profiles.length > 1}>
              <DropdownMenu.Separator class="menu-separator" />
              <DropdownMenu.Item
                class="menu-item menu-item-plain menu-item-danger"
                closeOnSelect={false}
                onSelect={() => void remove()}
              >
                <Trash2 size={17} /> {confirmDelete() ? "Bấm lần nữa để xóa hồ sơ" : "Xóa hồ sơ này"}
              </DropdownMenu.Item>
            </Show>
            <Show when={error()}><p class="menu-error">{error()}</p></Show>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu>

      <ProfileNameDialog
        open={dialog() !== undefined}
        mode={dialog() ?? "rename"}
        profile={props.active}
        onClose={() => setDialog(undefined)}
        onDone={props.onChanged}
      />
    </>
  );
}
