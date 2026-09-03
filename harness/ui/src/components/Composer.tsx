import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js";
import { useDragDrop } from "../hooks/useDragDrop";
import { type Attachment, pickFiles, resolveAttachments } from "../lib/attach";
import { applyCompletion, completePaths, findTrigger, rankCommands } from "../lib/complete";
import CompletionPopup, { type Suggestion } from "./CompletionPopup";
import { displayMode } from "../lib/prefs";
import { notify } from "../lib/toast";
import type { ModelChoice, ProjectKind, ToolScope } from "../lib/protocol";
import Icon, { type IconName } from "./Icon";
import Menu from "./Menu";
import ModelPicker from "./ModelPicker";
import { IconButton } from "./primitives";

/**
 * Biểu tượng của từng lệnh `/`.
 *
 * Ở đây chứ không trong `lib/complete.ts`: bộ biểu tượng là chuyện của tầng vẽ, và danh
 * sách lệnh phải kiểm chứng được mà không cần biết ứng dụng vẽ chúng bằng hình gì. Hình
 * gánh phần nghĩa mà câu mô tả một dòng phải bỏ lại.
 */
const COMMAND_ICON: Record<string, IconName> = {
  moi: "plus",
  tim: "search",
  duan: "folder",
  thaydoi: "diff",
  taplieu: "library",
  mohinh: "model",
  mcp: "plug",
  quyen: "hand",
  phimtat: "enter",
  caidat: "settings",
};

const SCOPE_LABEL: Record<ToolScope, string> = {
  read: "Chỉ đọc",
  write: "Đọc và ghi",
  shell: "Đọc, ghi và chạy lệnh",
};

/**
 * Một mảnh của dòng trạng thái dưới ô soạn tin.
 *
 * Khai ở đây chứ không lôi vào `primitives.tsx`: hình dạng này chỉ đúng cho đúng một dòng
 * trong toàn ứng dụng, và một primitive dùng chung mà chỉ có một chỗ gọi là một primitive
 * chưa biết mình phải làm gì.
 *
 * `note` là phần phụ chìm hơn một bậc — loại dự án đứng sau tên dự án — chứ không phải một
 * mảnh riêng: nó không tự đứng một mình mà có nghĩa. `warn` chỉ dành cho mảnh **đang** đáng
 * lo, không phải cho mọi mảnh có con số.
 */
type MetaBit = { icon: IconName; text: string; note?: string; warn?: boolean };

/**
 * Ô soạn tin: một khối bo tròn nằm giữa dưới, và **mọi** nút nằm trong viền của nó.
 *
 * Được điều khiển từ ngoài (`value`/`onChange`) vì kéo thả tệp phải chèn được đường dẫn
 * vào bản nháp, và bản nháp phải sống sót qua việc đổi phiên.
 *
 * Hàng công cụ nằm *trong* khung viền chứ không ở trên nó: mô hình và phạm vi tool là
 * thuộc tính của tin nhắn sắp gửi, không phải của cả màn hình, và đặt chúng ngoài khung
 * là mời người dùng quên mất chúng tồn tại. Cùng lý do khiến ChatGPT dời bộ chọn mô hình
 * từ thanh trên xuống đây.
 *
 * Phạm vi tool thì **không** giấu sau menu "+" như ChatGPT giấu tool của nó: chọn "chạy
 * lệnh" là cấp cho mô hình quyền chạy lệnh trên máy này, và một quyền đang mở phải đọc
 * được mà không cần bấm vào đâu cả. Cùng luật đó bắt nó phải *tắt đi trông thấy* khi chưa
 * có dự án — xem `hasProject`.
 *
 * Mức đang hiện là **mức sẽ đi kèm lượt kế**, không phải một thiết lập được lưu ở đâu đó:
 * mỗi lần gửi, giá trị này đi thẳng vào `send_message` và lõi siết sổ đăng ký tool theo
 * nó cho đúng lượt ấy. Nhãn ở đây vì vậy đọc được như một lời hứa kiểm chứng được, chứ
 * không phải một cái công tắc trang trí.
 *
 * Mọi thứ quanh ô nhập xếp thành **ba tầng**, và tầng quyết định hình dạng chứ không phải
 * chỗ ngồi:
 *
 *  1. Thứ *bấm được và đổi được lượt kế* — quyền, mô hình, nút Gửi — mang hình viên thuốc
 *     có nền riêng. Chúng luôn có mặt, kể cả khi giá trị đang là mặc định.
 *  2. Thứ *chỉ để đọc* — dự án, MCP, ngữ cảnh — là chữ thường màu `--muted` trên một hàng
 *     duy nhất. Trước đây chúng cũng đeo viên thuốc, và một hàng sáu viên thuốc đồng hạng
 *     dạy mắt rằng cái nào cũng bấm được, cái nào cũng quan trọng ngang nhau — nên rốt cuộc
 *     không cái nào được đọc, kể cả cái quyền. Chữ vẫn nói đủ từng ấy thông tin.
 *  3. Thứ *chỉ đúng khi có việc* — cảnh báo mô hình, gợi ý đính kèm, "chưa mở dự án" — nằm
 *     trong một hàng biết xuống dòng và biến mất hẳn khi không có việc gì.
 *
 * Ba tầng ấy là cách hạ nhiễu **mà không giấu gì**: không có thông tin nào lui vào sau một
 * nút phải bấm mới thấy, chỉ có thông tin bị hạ xuống đúng giọng của nó.
 */
export default function Composer(props: {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  /**
   * Khoá cứng ô nhập. Chỉ dùng cho việc đổi dự án — lúc đó mọi thứ trên màn hình còn nói
   * về dự án cũ. **Không** dùng cho "trợ lý đang trả lời": gõ tiếp trong lúc chờ là việc
   * bình thường, và câu vừa gõ đi vào `queued`.
   */
  disabled: boolean;
  busy: boolean;
  /** Câu đang chờ lượt hiện tại kết thúc. Chuỗi rỗng nghĩa là không có gì chờ. */
  queued?: string;
  /** Bỏ câu đang chờ. */
  onUnqueue?: () => void;
  onStop: () => void;
  model: string;
  models: ModelChoice[];
  onPickModel: (model: string) => void;
  /** Mở cài đặt → nhà cung cấp mô hình, từ trong bộ chọn mô hình. */
  onManageProviders: () => void;
  /** Câu cảnh báo dưới ô chọn mô hình. `undefined` khi không có gì phải nói. */
  modelWarning?: string;
  scope: ToolScope;
  onPickScope: (scope: ToolScope) => void;
  /**
   * Có dự án đang mở hay không.
   *
   * Không có thì lõi **không cắm tool nào** của tầng dự án, nên cả ba mức phạm vi đều
   * không có gì đằng sau. Bộ chọn phải nói ra điều đó thay vì đứng yên trông như đang bật:
   * một quyền trông như đang mở mà thực ra rỗng là kiểu nói dối tệ nhất một giao diện
   * quyền hạn làm được — người dùng dựa vào nó để quyết định có gửi câu tiếp theo không.
   */
  hasProject: boolean;
  /** Tên dự án đang mở, cho dòng trạng thái dưới ô soạn tin. */
  projectName?: string;
  projectKind?: ProjectKind;
  /**
   * Số server MCP **đang nối** — không phải số server đã khai báo.
   *
   * `0` không được viết ra thành chữ: xem `meta` bên dưới, chỗ dựng dòng trạng thái.
   */
  mcpConnected: number;
  /** Còn thứ khác nằm dưới ô soạn tin (mấy câu gợi ý), nên đáy thu lại một nấc. */
  moreBelow?: boolean;
  /** Chạy một lệnh `/`. Không truyền thì bộ lệnh không mở. */
  onCommand?: (name: string) => void;
  /**
   * Ngữ cảnh đã dùng ở bước gần nhất. `null` khi lượt nào cũng chưa chạy trong phiên này.
   *
   * `window` là `null` khi không hỏi được cửa sổ của mô hình — khi ấy chip chỉ hiện con số
   * token, không hiện phần trăm: một tỉ lệ không có mẫu số là một con số bịa.
   */
  usage?: { used: number; window: number | null } | null;
}) {
  let composing = false;
  let field: HTMLTextAreaElement | undefined;
  const [focused, setFocused] = createSignal(false);

  // ---- hoàn thành `@` và `/` ------------------------------------------------
  //
  // Con trỏ giữ ở đây chứ không đọc thẳng từ `field.selectionStart` mỗi lần vẽ: đọc thẳng
  // là đọc DOM trong lúc dựng, và Solid không vẽ lại khi con trỏ dịch mà chữ không đổi.
  const [caret, setCaret] = createSignal(0);
  const [dismissed, setDismissed] = createSignal(false);
  const [cursor, setCursor] = createSignal(0);

  const trigger = createMemo(() => {
    if (dismissed()) return null;
    const found = findTrigger(props.value, caret());
    if (found?.kind === "command" && props.onCommand === undefined) return null;
    return found;
  });

  // Chỉ hỏi lõi khi đang thật sự gõ một đường dẫn. `createResource` gộp các lần gõ liên
  // tiếp: lần gọi cũ bị bỏ khi truy vấn đổi, nên gõ nhanh không xếp thành một hàng đợi.
  const [paths] = createResource(
    () => (trigger()?.kind === "path" ? trigger()!.query : null),
    (query) => completePaths(query, 8),
  );

  const items = createMemo<Suggestion[]>(() => {
    const found = trigger();
    if (!found) return [];
    if (found.kind === "command") {
      return rankCommands(found.query).map((command) => ({
        value: command.name,
        label: `/${command.name}`,
        icon: COMMAND_ICON[command.name] ?? "terminal",
        hint:
          command.needsProject === true && !props.hasProject ? "cần một dự án" : command.hint,
        disabled: command.needsProject === true && !props.hasProject,
      }));
    }
    return (paths() ?? []).map((path) => ({ value: path }));
  });

  // Truy vấn đổi thì con trỏ về đầu: giữ nguyên chỉ số cũ là để nó trỏ vào một hàng khác
  // hẳn sau khi danh sách đã thay, và Enter chèn thứ người dùng không nhìn.
  createEffect(() => {
    items();
    setCursor(0);
  });

  const open = () => trigger() !== null && items().length > 0;

  const moveCursor = (delta: number) => {
    const count = items().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  /** Ghi lại chỗ con trỏ sau khi trình duyệt đã dịch nó. */
  const syncCaret = (el: HTMLTextAreaElement) => setCaret(el.selectionStart ?? 0);

  const choose = (item: Suggestion) => {
    const found = trigger();
    if (!found || item.disabled === true) return;
    if (found.kind === "command") {
      props.onChange("");
      setDismissed(true);
      props.onCommand?.(item.value);
      field?.focus();
      return;
    }
    const next = applyCompletion(props.value, found, item.value);
    props.onChange(next.text);
    // Đặt lại con trỏ **sau** khi Solid ghi giá trị mới xuống DOM, nếu không trình duyệt
    // đẩy nó về cuối chuỗi và người dùng mất chỗ đang gõ giữa câu.
    queueMicrotask(() => {
      if (!field) return;
      field.setSelectionRange(next.caret, next.caret);
      setCaret(next.caret);
      field.focus();
    });
  };

  const optionId = (index: number) => `composer-opt-${index}`;

  /** Tỉ lệ lấp đầy ngữ cảnh, hoặc `null` khi chưa đáng nói ra. */
  const contextPressure = createMemo(() => {
    const counted = props.usage;
    if (!counted || counted.window === null || counted.window <= 0) return null;
    const ratio = counted.used / counted.window;
    return ratio >= 0.6 ? { ratio: Math.min(ratio, 1) } : null;
  });

  /**
   * Dòng trạng thái dưới ô soạn tin: **lượt sắp gửi sẽ chạy với những gì**.
   *
   * Dựng thành mảng chứ không viết thẳng ba khối JSX cạnh nhau, vì dấu `·` chỉ được đứng
   * *giữa* hai mảnh có thật — viết tay thì mỗi lần một mảnh vắng mặt lại còn một dấu chấm
   * treo lơ lửng ở đầu hoặc cuối dòng.
   *
   * Mảnh nào **chỉ có ý nghĩa khi khác mặc định** thì vắng mặt ở mặc định. "0 server MCP"
   * và "chưa có dự án" là hai câu trả lời "không" mà sự vắng mặt cũng nói được, còn quyền
   * và mô hình thì luôn hiện ở tầng trên — hai thứ ấy không có ngoại lệ nào cả. Riêng
   * "chưa có dự án" còn được nói thành câu đầy đủ ở tầng cảnh báo, kèm cả hệ quả của nó,
   * nên lặp lại ở đây chỉ là lặp.
   */
  const meta = createMemo<MetaBit[]>(() => {
    const rows: MetaBit[] = [];

    if (props.hasProject) {
      rows.push({
        icon: "folder-open",
        text: props.projectName ?? "Dự án",
        note: props.projectKind === "docs" ? "tài liệu" : "mã nguồn",
      });
    }

    // Số server, không phải tên: dòng này chỉ cần trả lời "có thêm tool không". Ai muốn
    // biết những tool nào thì đã có trang Server MCP, và nhét bốn cái tên vào đây là đẩy
    // dòng trạng thái dài hơn cả câu người dùng sắp gõ.
    if (props.mcpConnected > 0) {
      rows.push({ icon: "plug", text: `${props.mcpConnected} server MCP` });
    }

    // Áp lực ngữ cảnh — **chỉ hiện khi đã đáng lo**.
    //
    // Ngưỡng 60% là có chủ ý: dưới mức đó con số không đổi được quyết định nào của người
    // dùng, và một con số đứng đó suốt phiên chỉ dạy mắt bỏ qua đúng chỗ mà về sau nó cần
    // nhìn. Trên mức đó thì nó trả lời một câu thật: còn bao nhiêu chỗ trước khi phần đầu
    // cuộc trò chuyện bị rút gọn.
    //
    // Mẫu số là cửa sổ mà **plugin nén** dùng làm ngưỡng, không phải cửa sổ của mô hình —
    // nên nó chạm mức cảnh báo đúng lúc nén sắp chạy, chứ không sau đó.
    const pressure = contextPressure();
    if (pressure) {
      rows.push({
        icon: "model",
        text: `Ngữ cảnh ${Math.round(pressure.ratio * 100)}%`,
        warn: pressure.ratio >= 0.85,
      });
    }

    return rows;
  });

  /**
   * Chèn đường dẫn vào bản nháp, sau khi lõi đã nhìn vào đĩa và duyệt từng cái một.
   *
   * Chèn vào **cuối** thay vì thay thế: người dùng thường đã gõ dở câu hỏi rồi mới đi tìm
   * tệp. Mỗi đường dẫn một dòng, vì đường dẫn có dấu cách trong đó và một danh sách ngăn
   * bằng dấu cách thì không tách lại được — cả người đọc lẫn mô hình.
   *
   * Một tệp bị từ chối **không** chặn những tệp còn lại: thả năm tệp mà một cái nằm ngoài
   * dự án thì bốn cái kia vẫn vào, và câu lỗi nói về đúng cái thứ năm. Bỏ cả lô vì một
   * đường dẫn hỏng là bắt người dùng làm lại một việc đã gần xong.
   */
  const attach = async (paths: string[]) => {
    if (paths.length === 0) return;

    let resolved: Attachment[];
    try {
      resolved = await resolveAttachments(paths);
    } catch (err) {
      // Lõi từ chối cả lô — gần như luôn là "chưa mở dự án". Nguyên văn từ lõi: chỉ nó
      // biết vì sao, và một câu ta tự viết ở đây sẽ đoán sai vào đúng lần nó đoán khác.
      notify("error", String(err));
      return;
    }

    const usable = resolved.filter((entry) => entry.error === null);
    const refused = resolved.filter((entry) => entry.error !== null);
    // Một thông báo cho cả lô, không phải một thông báo mỗi tệp: thả nhầm cả thư mục
    // Downloads vào đây thì hai mươi thẻ giống hệt nhau không nói được gì mà một thẻ không
    // nói được. Câu đầu là câu cụ thể — có tên tệp trong đó — rồi mới tới con số.
    if (refused.length > 0) {
      notify(
        "error",
        refused.length === 1
          ? refused[0]!.error!
          : `${refused[0]!.error} (và ${refused.length - 1} tệp nữa không đính kèm được)`,
      );
    }

    if (usable.length === 0) return;
    const prefix = props.value.trim() === "" ? "" : `${props.value.replace(/\s*$/, "")}\n`;
    props.onChange(`${prefix}${usable.map((entry) => entry.path).join("\n")}\n`);
    field?.focus();
  };

  /**
   * Kéo thả: cùng đường đi với nút đính kèm, không phải một đường riêng.
   *
   * `useDragDrop` phát cho **mọi** chỗ đang nghe, nên cú thả phải có đúng một chủ trên mỗi
   * màn hình. Ở hội thoại chủ đó là ô soạn tin. Trước đây vỏ ứng dụng cũng nghe cú thả ở
   * màn hình này và đem đường dẫn đi mở thành dự án, nên một cú thả làm hai việc: tệp thì
   * vừa được đính kèm vừa nhận một câu lỗi "không phải một thư mục", còn thư mục thì vừa
   * được đính kèm vừa âm thầm đổi cả dự án dưới chân phiên đang chạy.
   */
  useDragDrop((paths) => void attach(paths));

  /**
   * Lối vào thứ hai của cùng việc ấy: hộp thoại của hệ điều hành.
   *
   * Mỗi đường ra khỏi hàm này đều **nói một câu**, trừ đúng một đường: người dùng bấm Huỷ.
   * Đó là luật của một nút — bấm vào mà không có gì xảy ra và không có gì được nói ra thì
   * nút ấy hỏng, kể cả khi bên trong nó mọi thứ chạy đúng như đã viết.
   */
  const browse = async () => {
    // Chưa có dự án thì trả lời ngay, không mở hộp thoại: bắt người dùng đi chọn tệp rồi
    // mới nói là không nhận được nó là lấy công của họ để nói một câu đã biết trước.
    if (!props.hasProject) {
      notify("error", "Chưa mở dự án nên chưa đính kèm tệp được.");
      return;
    }
    try {
      const picked = await pickFiles();
      if (picked === null) {
        notify("error", "Hộp thoại chọn tệp chỉ có trong ứng dụng.");
        return;
      }
      await attach(picked);
    } catch (err) {
      notify("error", `Không mở được hộp thoại chọn tệp: ${err}`);
    }
  };

  // Không chặn khi `busy`: App nhận câu này và xếp nó vào ô chờ. Chặn ở đây thì Enter
  // giữa lượt lại không làm gì cả, đúng cái im lặng vừa bỏ đi.
  const submit = () => {
    if (props.disabled || props.value.trim() === "") return;
    props.onSubmit();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    // Bộ gõ tiếng Việt gửi Enter để chốt từ đang gõ. Không có guard này thì mỗi lần
    // chốt dấu là một lần gửi nhầm — chat_view.py:453 đã vấp đúng chỗ đó.
    if (composing || event.isComposing) return;

    // Danh sách gợi ý đang mở thì nó **giành trước** các phím điều hướng. Enter ở đây chèn
    // một gợi ý chứ không gửi tin: người vừa gõ `@sto` và thấy một danh sách đang chờ thì
    // Enter của họ nói về danh sách ấy, không nói về cả tin nhắn.
    if (open()) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        moveCursor(1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        moveCursor(-1);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        const item = items()[cursor()];
        if (item && item.disabled !== true) {
          event.preventDefault();
          choose(item);
          return;
        }
      }
      if (event.key === "Escape") {
        event.preventDefault();
        // Đóng danh sách, **giữ nguyên chữ**. Xoá luôn phần đã gõ là phạt người dùng vì đã
        // gõ một dấu `@` — và Esc ở mọi chỗ khác trong ứng dụng cũng chỉ đóng, không xoá.
        setDismissed(true);
        return;
      }
    }

    const chord = event.metaKey || event.ctrlKey;
    if (event.key === "Enter" && (chord || !event.shiftKey)) {
      event.preventDefault();
      submit();
    }
  };

  // Ô nhập cao theo nội dung, tối đa ~10 dòng. Đo bằng `scrollHeight` sau khi ép về 0:
  // không ép thì ô đã cao rồi sẽ không bao giờ thấp xuống lại khi người dùng xoá bớt.
  const resize = (el: HTMLTextAreaElement) => {
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  return (
    <form
      class="shrink-0 bg-bg px-(--page-pad-x) pt-sm"
      classList={{
        "pb-(--page-pad-y)": props.moreBelow !== true,
        "pb-sm": props.moreBelow === true,
      }}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      {/* Câu đang chờ, hiện **trên** ô soạn tin và rộng đúng bằng nó.

          Xếp hàng mà không hiện ra thì Enter đọc như một cú bấm rơi vào hư không, và người
          dùng gõ lại câu đó lần nữa. Kèm nút bỏ vì đổi ý giữa lúc chờ là chuyện thường —
          câu trả lời đang chảy ngay trên kia có thể vừa trả lời xong chính nó. */}
      <Show when={(props.queued ?? "") !== ""}>
        <div
          class="mx-auto mb-xs flex w-full items-center gap-xs rounded-panel border border-line bg-surface-soft px-md py-xs"
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <Icon name="clock" size={13} />
          <span class="shrink-0 text-xs text-faint">Gửi khi xong</span>
          <span class="min-w-0 flex-1 truncate text-sm text-text">{props.queued}</span>
          {/* Cao 28px chứ không co theo chữ: đây là lối thoát duy nhất khỏi hàng chờ, và
              một lối thoát rộng bằng hai chữ là một lối thoát bấm trượt. */}
          <button
            type="button"
            onClick={() => props.onUnqueue?.()}
            aria-label="Bỏ câu đang chờ"
            class="flex h-7 shrink-0 items-center rounded-btn px-2xs text-xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-text"
          >
            Bỏ
          </button>
        </div>
      </Show>

      <div
        // Ô soạn tin rộng đúng bằng cột chữ phía trên nó: lệch một chút thôi là mắt đọc
        // ra hai khối không thuộc về nhau.
        //
        // Lúc có tiêu điểm, viền đổi sang màu nhấn **và** một quầng sáng rất mỏng nở ra
        // quanh khung. Quầng ấy là chuyển động duy nhất của cả ô soạn tin, và nó có lý do:
        // ô này nằm sát đáy một cửa sổ đầy chữ, nên "con trỏ đang ở đâu" phải đọc được từ
        // khoé mắt chứ không phải bằng cách đi tìm cái nháy. Đổi mỗi màu viền là một pixel
        // đổi màu — ngoại vi của mắt không bắt được. `transition` (không phải
        // `transition-colors`) vì `box-shadow` phải trôi cùng nhịp với viền, nếu không
        // quầng bật ra trước rồi viền đuổi theo sau. Người chọn giảm chuyển động vẫn có
        // đủ hai tín hiệu — app.css cắt thời lượng chứ không cắt trạng thái cuối.
        class="relative mx-auto flex w-full flex-col rounded-composer border bg-surface shadow-float transition duration-[var(--dur-base)] ease-[var(--ease-out)]"
        classList={{
          "border-accent ring-[3px] ring-accent/15": focused(),
          "border-line-strong ring-0 ring-transparent": !focused(),
          "max-w-(--reading-measure)": displayMode() === "bubble",
          "max-w-[min(100%,980px)]": displayMode() === "document",
        }}
      >
        <CompletionPopup
          items={items()}
          cursor={cursor()}
          id="composer-completions"
          optionId={optionId}
          onPick={choose}
          onHover={setCursor}
          empty={
            trigger()?.kind === "path" && !paths.loading ? "Không có tệp nào khớp." : undefined
          }
        />

        <textarea
          ref={(el) => {
            field = el;
            queueMicrotask(() => resize(el));
          }}
          rows={1}
          value={props.value}
          disabled={props.disabled}
          placeholder={
            props.busy
              ? "Gõ câu tiếp theo…  (Enter để xếp hàng chờ)"
              : "Nhập…  (Enter để gửi, Shift+Enter xuống dòng)"
          }
          aria-label="Nội dung tin nhắn"
          aria-keyshortcuts="Enter Meta+Enter Control+Enter"
          onCompositionStart={() => (composing = true)}
          onCompositionEnd={() => (composing = false)}
          role="combobox"
          aria-expanded={open()}
          aria-controls="composer-completions"
          aria-activedescendant={open() ? optionId(cursor()) : undefined}
          aria-autocomplete="list"
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onInput={(event) => {
            props.onChange(event.currentTarget.value);
            resize(event.currentTarget);
            // Gõ tiếp sau khi đã Esc là một lời gọi mới, không phải phần đuôi của lời gọi
            // vừa bị đóng — nếu không, Esc một lần là tắt hoàn thành cho tới hết câu.
            setDismissed(false);
            syncCaret(event.currentTarget);
          }}
          onKeyUp={(event) => syncCaret(event.currentTarget)}
          onClick={(event) => syncCaret(event.currentTarget)}
          onKeyDown={onKeyDown}
          class="max-h-[220px] w-full resize-none bg-transparent px-md pt-md pb-2xs text-base text-text outline-none placeholder:text-faint"
        />

        {/* Tầng ba: mọi câu **chỉ đúng khi có việc**, gom vào **một hàng biết xuống dòng**
            chứ không xếp chồng thành ba dải.

            Ô soạn tin chỉ cao vài chục pixel, và mỗi dải chữ thêm vào đẩy hàng nút xuống
            một nấc — ba dải rời là ba nấc, và người dùng thấy đáy cửa sổ nhấp nhô mỗi lần
            một điều kiện bật tắt. Nằm cùng hàng thì hai câu ngắn ở chung một dòng, và chỉ
            khi cửa sổ hẹp chúng mới tự rơi xuống.

            `role="status"` chứ không `alert` ở cả hai: đây là những điều kiện đang tồn tại,
            không phải sự kiện vừa xảy ra — trình đọc màn hình nên đọc chúng khi tới lượt.

            Và vì thế **không có câu lỗi nào** ở đây. Đó là luật chứ không phải chỗ còn
            trống: một cú đính kèm hỏng là chuyện vừa xảy ra, nó chỉ đúng trong vài giây, và
            nó phải tự đi — nên nó ra thông báo nổi (`lib/toast.ts`), nơi nó vẫn nói được cả
            khi người dùng đã chuyển sang tab khác. Hàng này từng giữ một câu như thế, và
            cái giá là một dải chữ nhấp nháy theo từng cú bấm ngay dưới chỗ đang gõ. */}
        <Show when={!props.hasProject || props.modelWarning}>
          <div class="flex flex-wrap items-center gap-x-md gap-y-3xs px-md pb-2xs text-xs">
            {/* Câu chốt lại bằng "vẫn gửi được": đây là một giới hạn đang tồn tại, không
                phải một thứ vừa hỏng, và người đọc phải rời câu này với niềm tin rằng ô
                soạn tin bên dưới còn dùng được.

                Đây cũng là **chỗ duy nhất** nói "chưa có dự án" bằng chữ, kể từ khi dòng
                trạng thái phía dưới thôi mang một viên thuốc "Chưa có dự án" chỉ để nói
                đúng chừng ấy mà không nói được hệ quả. */}
            <Show when={!props.hasProject}>
              <p class="m-0 flex items-center gap-2xs text-muted" role="status">
                <Icon name="tools" size={12} />
                Chưa có dự án: chưa có tool, vẫn gửi được.
              </p>
            </Show>

            <Show when={props.modelWarning}>
              {(message) => (
                <p class="m-0 flex items-center gap-2xs text-warn" role="status">
                  <Icon name="warn" size={12} />
                  {message()}
                </p>
              )}
            </Show>
          </div>
        </Show>

        {/* Tầng một, và là hàng duy nhất còn đeo viên thuốc: đính kèm ở mép trái, **quyền**
            ngay cạnh nó, và mô hình dạt sang phải cạnh nút Gửi. Thứ tự này không tuỳ tiện —
            trái sang phải là "đưa gì vào → được làm gì với nó → ai làm", và cái đắt nhất
            trong ba cái đó là quyền, nên nó đứng ở chỗ mắt chạm tới trước.

            Bốn thứ ở đây đều **đổi được lượt kế**, và cả bốn đều cao 32px nên vẫn bấm trúng
            bằng ngón tay. Dự án/MCP/ngữ cảnh đã rời khỏi hình viên thuốc chính vì không đổi
            được gì cả: chúng chỉ báo cáo lại một lựa chọn đã làm ở nơi khác. */}
        <div class="flex flex-wrap items-center gap-2xs px-2xs pb-2xs">
          {/* Nút này **mở hộp thoại chọn tệp**, không mở một câu giải thích.

              Nó từng chỉ bật tắt một dòng chữ hướng dẫn kéo thả, với lý do rằng chỉ tầng
              hệ điều hành mới đưa được đường dẫn tuyệt đối. Vế sau đúng, vế trước sai:
              hộp thoại của Tauri *là* tầng hệ điều hành và trả về đúng đường dẫn ấy — thư
              viện tài liệu đã dùng nó từ đầu. Cái không đưa được đường dẫn là
              `<input type="file">` của trình duyệt, không phải mọi hộp thoại.

              Kéo thả vẫn còn, và ở lại đúng vai của nó: một lối tắt cho người đang mở sẵn
              một cửa sổ thư mục, chứ không phải cử chỉ duy nhất mở được đường vào. Nó được
              nhắc trong `aria-label` và trong chú giải, tức ở chỗ người ta hỏi "nút này
              làm gì", chứ không chiếm một dòng thường trực dưới ô soạn tin.

              **Không** tắt khi chưa mở dự án, dù lúc ấy chẳng tệp nào đính kèm được. Một
              nút xám không nói được vì sao nó xám; người dùng bấm, không thấy gì, và học
              được đúng một điều — cái nút này hỏng. Nó ở lại bấm được và trả lời bằng chữ,
              ngay trên đầu nó. Cùng luật với nút dừng trong bản demo (`lib/agent.ts`). */}
          <IconButton
            icon="paperclip"
            label="Đính kèm tệp, hoặc kéo thả vào cửa sổ"
            onClick={() => void browse()}
          />

          {/* Vô hiệu chứ không ẩn hẳn: chỗ ngồi của bộ chọn giữ nguyên qua hai trạng thái,
              nên người vừa đóng dự án nhìn thấy *cái gì đã đổi* thay vì thấy một nút biến
              mất. Nó ra khỏi vòng Tab luôn — không còn lựa chọn nào để đi tới, và lý do
              nằm ở dòng chữ ngay trên, chỗ trình đọc màn hình cũng đọc được.

              Nói "chưa dùng được" bằng chữ chứ **không gạch ngang** cái nhãn: chữ bị gạch
              đọc ra là một thứ vừa hỏng hoặc vừa bị bỏ đi, mà đây là một quyền đang tắt vì
              chưa có gì để cấp. "Chưa xong" và "hỏng" là hai trạng thái.

              Cỡ chữ ở đây bám `text-xs` để khớp *đúng từng pixel* với viên thuốc thật do
              `Menu variant="pill"` vẽ ra — chỗ ngồi chỉ giữ nguyên nếu cả bề cao lẫn bề
              ngang đều giữ nguyên, và bề ngang đi theo cỡ chữ. Đổi một bên là phải đổi
              bên kia cùng lúc, kể cả bộ chọn mô hình đứng cạnh. */}
          <Show
            when={props.hasProject}
            fallback={
              <span
                aria-hidden="true"
                class="flex h-(--control-h) items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-sm text-xs text-faint"
              >
                <Icon name="hand" size={13} />
                {SCOPE_LABEL[props.scope]}
                <span class="opacity-70">· chưa dùng được</span>
              </span>
            }
          >
            {/* Bàn tay thay cho cái cờ lê: cờ lê nói "có công cụ", còn hàng này nói **được
                phép làm tới đâu** — cùng một hình cho hai ý khác nhau là chỗ người ta đọc
                lướt qua rồi tưởng mình đã hiểu. */}
            <Menu
              variant="pill"
              placement="up"
              align="left"
              icon="hand"
              text={SCOPE_LABEL[props.scope]}
              tone={props.scope === "shell" ? "warn" : "neutral"}
              label={`Phạm vi tool: ${SCOPE_LABEL[props.scope]}`}
              items={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
                id: scope,
                label: SCOPE_LABEL[scope],
                icon: "hand" as const,
                onSelect: () => props.onPickScope(scope),
              }))}
            />
          </Show>

          <span class="flex-1" />

          <ModelPicker
            value={props.model}
            models={props.models}
            onPick={props.onPickModel}
            onManageProviders={props.onManageProviders}
          />

          <Show
            when={props.busy}
            fallback={
              <button
                type="submit"
                disabled={props.disabled || props.value.trim() === ""}
                class="flex h-(--control-h) items-center gap-2xs rounded-pill bg-accent px-md text-sm font-medium text-on-accent transition-colors duration-[var(--dur-fast)] hover:bg-accent-hover disabled:opacity-40"
              >
                <Icon name="send" size={14} />
              </button>
            }
          >
            <button
              type="button"
              onClick={props.onStop}
              class="flex h-(--control-h) items-center gap-2xs rounded-pill border border-line-strong px-md text-sm font-medium text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="stop" size={14} />
            </button>
          </Show>
        </div>
      </div>

      {/* Tầng hai — dòng trạng thái: **lượt sắp gửi sẽ chạy với những gì**.

          Nó ở ngoài viền ô soạn tin chứ không trong: mọi thứ trong viền đều bấm được và đổi
          được, còn mấy mảnh này chỉ báo cáo lại trạng thái đã chọn ở nơi khác. Trộn hai loại
          vào một khung là mời người dùng bấm vào một cái nhãn — và đó chính là lời mời mà
          hình viên thuốc cũ phát ra: nền riêng, góc bo tròn, đứng ngay dưới ba cái nút thật.

          Nên bây giờ chúng là **chữ**: `--muted`, không nền, không viền, cách nhau bằng dấu
          `·`, biểu tượng nhỏ giữ lại để nhận ra từng mảnh mà không phải đọc. Chữ vẫn nói đủ
          những câu người ta thật sự hỏi trước khi bấm Gửi — nó đọc thư mục nào, có thêm tool
          nào ngoài tool dựng sẵn — chỉ là nói bằng giọng của một dòng chân trang, đúng hạng
          của nó.

          `text-xs` chứ không nhỏ hơn: hạ hạng không có nghĩa là hạ tới mức phải nheo mắt,
          và đây là dòng duy nhất nói ra thư mục mà trợ lý sắp đọc.

          Cố ý **không** có mảnh mô hình ở đây. Tên mô hình đã nằm trong bộ chọn ngay phía
          trên, cách chưa tới ba mươi pixel; lặp lại nó chỉ làm dài thêm một dòng vốn tồn tại
          để trả lời những câu bộ chọn *không* trả lời được. */}
      <Show when={meta().length > 0}>
        <div
          role="group"
          aria-label="Lượt kế sẽ chạy với"
          class="mx-auto mt-xs flex w-full flex-wrap items-center gap-x-2xs gap-y-3xs px-md text-xs text-muted"
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <For each={meta()}>
            {(item, index) => (
              <span
                class="inline-flex items-center gap-2xs"
                classList={{ "text-warn": item.warn === true }}
              >
                {/* Dấu phân cách `aria-hidden`: mắt cần nó để tách hai mảnh, còn trình đọc
                    màn hình đã có khoảng nghỉ giữa hai phần tử rồi — đọc thêm "chấm giữa"
                    vào giữa mỗi mảnh là biến một dòng ngắn thành một câu lắp bắp.

                    Nó nằm *trong* mảnh chứ không đứng riêng để `flex-wrap` không bao giờ
                    bỏ một dấu chấm trơ trọi ở cuối dòng trên. Màu `--faint` đè lên cả màu
                    cảnh báo của mảnh: dấu ngăn cách không phải thứ đang cảnh báo. */}
                <Show when={index() > 0}>
                  <span aria-hidden="true" class="text-faint">
                    ·
                  </span>
                </Show>
                <Icon name={item.icon} size={12} />
                {item.text}
                <Show when={item.note}>
                  {(note) => <span class="text-faint">{note()}</span>}
                </Show>
              </span>
            )}
          </For>
        </div>
      </Show>
    </form>
  );
}
