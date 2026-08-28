import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { Boxes, Check, ChevronDown, Settings2 } from "lucide-solid";
import { For, Show } from "solid-js";
import { formatBytes } from "../format";
import type { ModelInfo } from "../types";

const shortName = (name: string) => name.replace(/:latest$/, "");

const stateLabel = (state: ModelInfo["state"]) => {
  switch (state) {
    case "loaded": return "đang trong bộ nhớ";
    case "installed": return "đã cài đặt";
    case "unloaded": return "chưa nạp";
    case "downloading": return "đang tải";
    case "failed": return "lỗi";
  }
};

const modelMeta = (model: ModelInfo) => {
  const parts = [model.runtime];
  if (model.size_bytes) parts.push(formatBytes(model.size_bytes));
  if (model.quantization) parts.push(model.quantization);
  if (model.capabilities.includes("vision")) parts.push("Đọc ảnh");
  parts.push(stateLabel(model.state));
  return parts.join(" · ");
};

export function ModelPicker(props: {
  models: ModelInfo[];
  selected: string;
  loading: boolean;
  onSelect: (name: string) => void;
  onManage: () => void;
}) {
  const label = () =>
    props.selected
      ? shortName(props.selected)
      : props.loading
        ? "Đang tải mô hình…"
        : "Chưa có mô hình";

  return (
    <DropdownMenu placement="top-start" gutter={10}>
      <DropdownMenu.Trigger
        type="button"
        class="model-picker-trigger"
        aria-label={`Mô hình: ${label()}`}
      >
        <Boxes size={17} />
        <span class="model-picker-name">{label()}</span>
        <ChevronDown size={15} aria-hidden="true" />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="model-picker-menu">
          <p class="model-picker-heading">Chọn mô hình</p>
          <Show
            when={props.models.length > 0}
            fallback={
              <p class="model-picker-empty">
                {props.loading ? "Đang đọc danh sách mô hình…" : "Chưa có mô hình trò chuyện nào."}
              </p>
            }
          >
            <DropdownMenu.RadioGroup
              class="model-picker-list"
              value={props.selected}
              onChange={props.onSelect}
            >
              <For each={props.models}>{(model) => (
                <DropdownMenu.RadioItem
                  class="model-option"
                  value={model.name}
                  closeOnSelect
                  disabled={model.state === "failed" || model.state === "downloading"}
                  aria-label={`${shortName(model.name)}, ${modelMeta(model)}`}
                >
                  <span class="model-option-copy">
                    <strong>{shortName(model.name)}</strong>
                    <small>{modelMeta(model)}</small>
                  </span>
                  <Show when={model.state === "loaded"}>
                    <span class="model-option-live" title="Đang nằm trong bộ nhớ" />
                  </Show>
                  <DropdownMenu.ItemIndicator class="model-option-check">
                    <Check size={16} />
                  </DropdownMenu.ItemIndicator>
                </DropdownMenu.RadioItem>
              )}</For>
            </DropdownMenu.RadioGroup>
          </Show>
          <DropdownMenu.Item class="model-picker-manage" onSelect={props.onManage}>
            <Settings2 size={16} />
            <span>Quản lý mô hình</span>
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  );
}
