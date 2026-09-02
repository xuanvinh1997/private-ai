import { createEffect, createMemo, createSignal, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import {
  answerApproval,
  cancelTurn,
  createSession,
  deleteSession,
  inTauri,
  listModels,
  listSessions,
  loadSession,
  renameSession,
  sendMessage,
} from "./lib/agent";
import { changedFiles } from "./lib/changes";
import { createConversation, nodesFromHistory } from "./lib/conversation";
import {
  demoKnobs,
  demoModels,
  demoNodes,
  demoParked,
  demoProjects,
  demoSessions,
  isDemo,
  runDemoTurn,
} from "./lib/demo";
import { listMcpServers } from "./lib/mcp";
import { changesPanelOpen, defaultToolScope, setChangesPanelOpen, setDisplayMode, setSidebarOpen, sidebarOpen } from "./lib/prefs";
import {
  closeProject,
  folderName,
  listProjects,
  openProject,
  removeProject,
  setProjectKind,
} from "./lib/projects";
import { titleFromMessage } from "./lib/sessions";
import type {
  AgentEvent,
  ApprovalDecision,
  ConversationNode,
  McpServer,
  ModelChoice,
  Project,
  ProjectKind,
  SessionSummary,
  ToolScope,
} from "./lib/protocol";
import { setTheme } from "./lib/theme";
import { TranscriptActionsProvider } from "./lib/transcriptActions";
import { useDragDrop } from "./hooks/useDragDrop";
import ApprovalDialog from "./components/ApprovalDialog";
import ChangesPanel, { ChangesBoard } from "./components/ChangesPanel";
import Composer from "./components/Composer";
import { EmptyLead, PromptChips } from "./components/EmptyState";
import { usableForChat } from "./components/ModelPicker";
import ProjectSwitcher from "./components/ProjectSwitcher";
import Sidebar, { projectTabs, tabsFor, type TabId } from "./components/Sidebar";
import ProjectsView from "./components/projects/ProjectsView";
import DocsView from "./components/docs/DocsView";
import SessionPalette from "./components/SessionPalette";
import SettingsView, { type SettingsPage } from "./components/SettingsView";
import Transcript from "./components/Transcript";
import WorkspaceHeader from "./components/WorkspaceHeader";

// Nạp sổ đăng ký renderer. Import vì hiệu ứng phụ là cố ý: đây là chỗ *duy nhất* biết
// danh sách renderer, nên thêm một loại node mới không kéo theo sửa đổi ở nơi nào khác.
import "./components/nodes";

/** Mô hình dùng khi chưa hỏi được máy chủ. Chỉ để ô chọn không trống. */
const MODEL_CHUA_BIET = "(chưa hỏi được máy chủ)";

/**
 * Vỏ ứng dụng: một thanh bên trái, một cột hội thoại căn giữa, và một bảng thay đổi
 * mở/đóng được ở bên phải.
 *
 * Hình dạng lấy từ ChatGPT và Codex, không từ LobeChat: **một** cột điều hướng thay vì
 * rail cộng panel, bộ chọn mô hình nằm trong ô soạn tin thay vì trên thanh tiêu đề, và
 * không có màn hình nào đọc mã nguồn — người dùng đã có editor của họ rồi. Thứ duy nhất
 * ứng dụng này thêm vào so với hình mẫu là quản lý nhà cung cấp mô hình.
 *
 * Trạng thái hội thoại nằm trong một store riêng cho từng phiên và được nhớ lại khi quay
 * về — chuyển phiên rồi mất chỗ đang đọc là cách nhanh nhất làm người ta ngại chuyển phiên.
 */
export default function App() {
  const conversation = createConversation();
  const [sessions, setSessions] = createSignal<SessionSummary[]>([]);
  const [currentId, setCurrentId] = createSignal("phien-nhap");
  const [draft, setDraft] = createSignal("");
  /**
   * Tin nhắn gõ **trong lúc lượt trước còn chạy**, chờ tới lượt nó.
   *
   * Trước đây ô soạn tin bị khoá suốt lượt, nên nghĩ ra câu hỏi tiếp theo giữa chừng là
   * phải giữ nó trong đầu cho tới khi trợ lý nói xong. Một agent chạy vài chục giây mỗi
   * lượt thì đó là vài chục giây người dùng không làm được gì.
   *
   * Đúng **một** ô chờ, không phải một hàng đợi: gửi liên tiếp ba câu vào một lượt đang
   * chạy là ba câu hỏi trên một ngữ cảnh mà người gõ chưa đọc, và câu thứ ba gần như luôn
   * là câu họ sẽ viết khác đi nếu đọc câu trả lời trước. Gõ tiếp thì thay ô chờ.
   */
  const [queued, setQueued] = createSignal("");
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [tab, setTab] = createSignal<TabId>("chat");
  // Trang cài đặt do đây giữ, không do `SettingsView` giữ: thanh bên có một hàng đi thẳng
  // tới trang MCP, và một trạng thái nằm trong màn hình con sẽ bỏ qua cú bấm ấy mỗi khi
  // màn hình cài đặt đã mở sẵn.
  const [settingsPage, setSettingsPage] = createSignal<SettingsPage>("chung");

  // Đổi dự án có thể làm chính màn hình đang mở biến mất khỏi thanh bên — mở một thư viện
  // tài liệu trong lúc đang đứng ở màn hình Thay đổi chẳng hạn. Không sửa thì thanh bên
  // không còn hàng nào sáng và khung nội dung trống trơn, trông y hệt một lỗi vẽ.
  createEffect(() => {
    if (!tabsFor(project()?.kind).includes(tab())) setTab("chat");
  });
  const [loading, setLoading] = createSignal(true);
  const [models, setModels] = createSignal<ModelChoice[]>([]);
  const [model, setModel] = createSignal(MODEL_CHUA_BIET);
  // Phiên đang được nạp lại từ sổ. Giữ id chứ không giữ boolean: chuyển phiên nhanh hai
  // lần thì kết quả về sau của phiên cũ không được ghi đè lên phiên mới.
  const [loadingSession, setLoadingSession] = createSignal<string | null>(null);
  const [loadError, setLoadError] = createSignal<string | null>(null);
  // Phạm vi tool của **lượt kế**: nó đi kèm mỗi lần gửi và không được ghi lại ở đâu.
  //
  // Điểm xuất phát thì đến từ trang Quyền (`defaultToolScope`), và đó là chỗ *duy nhất*
  // ghi được. Ranh giới ấy quan trọng: đổi mức trong ô soạn tin là một quyết định cho
  // lượt sắp gửi và nó chết theo cửa sổ, còn đổi mức ở trang Quyền là một quyết định về
  // mọi lượt sau này. Gộp hai thứ lại — cho ô soạn tin ghi ngược vào thiết lập — thì một
  // lần bật shell cho đúng một câu hỏi sẽ lặng lẽ ở lại đó mãi.
  const [scope, setScope] = createSignal<ToolScope>(defaultToolScope());

  const [projects, setProjects] = createSignal<Project[]>([]);
  // Đổi dự án là lõi tháo và cắm lại cả một nhánh plugin. Trong lúc đó mọi thứ trên màn
  // hình còn nói về dự án cũ, nên cờ này khoá thao tác thay vì chỉ hiện một cái chấm quay.
  const [switching, setSwitching] = createSignal(false);
  // Hàng dự án nào đang mở menu ngữ cảnh. Giữ id chứ không giữ boolean: mỗi dự án có menu
  // riêng, và một cờ chung sẽ mở cả mấy cái cùng lúc.
  const [projectMenu, setProjectMenu] = createSignal<string | null>(null);

  // Server MCP là "plugin" của ứng dụng này: mỗi cái cắm thêm một rổ tool vào lượt kế. Số
  // **đang nối** đứng trên hàng điều hướng và trong dải ngữ cảnh, nên nó phải là số thật
  // chứ không phải số server đã khai báo — một server `failed` không cho thêm tool nào.
  const [mcpServers, setMcpServers] = createSignal<McpServer[]>([]);
  const mcpConnected = () => mcpServers().filter((server) => server.state === "connected").length;
  // Hỏi lại mỗi lần quay về hội thoại, không chỉ một lần lúc mở app: trang cài đặt ngay
  // cạnh đây bật/tắt và cắm lại server được, và một con số đứng im sau đó là một lời nói
  // sai về chỗ trợ lý sắp lấy tool ra dùng.
  createEffect(() => {
    if (tab() === "chat") void refreshMcp();
  });
  async function refreshMcp() {
    setMcpServers(await listMcpServers());
  }

  const project = () => projects().find((entry) => entry.isCurrent) ?? null;
  const projectKey = () => project()?.id ?? "khong-co-du-an";
  /**
   * Có dự án đang mở hay không — và đây là một trạng thái hợp lệ, không phải một lần nạp
   * chưa xong.
   *
   * Lõi không tự nhận thư mục hiện hành làm dự án nữa, nên mở ứng dụng lần đầu là rơi
   * thẳng vào đây. Không có dự án thì **không plugin nào của tầng dự án được cắm**: còn
   * đúng `todo_write` cộng tool từ server MCP, và hội thoại chạy bình thường. Mọi màn
   * hình hứa hẹn đọc/sửa/chạy lệnh phải đọc cờ này chứ không tự đoán từ `kind`.
   */
  const hasProject = () => project() !== null;

  // Bản ghi của phiên không mở. Giữ trong bộ nhớ thôi: nguồn sự thật là sổ tay phiên
  // bên Rust, đây chỉ là bộ đệm để chuyển qua lại không phải nạp lại.
  const parked = new Map<string, ConversationNode[]>();

  const files = createMemo(() => changedFiles(conversation.nodes()));

  /**
   * Dòng phụ của một hàng phiên: câu cuối cùng đã nói trong phiên đó.
   *
   * Ưu tiên bản ghi **đang mở** hơn bản từ lõi: lượt vừa chạy xong chưa kịp vào danh sách,
   * và một dòng phụ nói về câu áp chót thì trông như giao diện bị treo. Lõi trả lời cho
   * mọi phiên còn lại, kể cả phiên chưa mở lần nào — đó là thứ bộ nhớ không làm được.
   */
  function preview(session: SessionSummary): string | undefined {
    const nodes = session.id === currentId() ? conversation.nodes() : parked.get(session.id);
    if (!nodes) return session.preview ?? undefined;
    for (let at = nodes.length - 1; at >= 0; at -= 1) {
      const node = nodes[at]!;
      if (node.kind === "user") return `Bạn: ${node.text}`;
      if (node.kind === "assistant" && node.text !== "") return node.text;
    }
    return session.preview ?? undefined;
  }

  onMount(async () => {
    if (isDemo()) {
      const knobs = demoKnobs();
      if (knobs.theme) setTheme(knobs.theme);
      if (knobs.mode) setDisplayMode(knobs.mode);
      if (knobs.changes !== undefined) setChangesPanelOpen(knobs.changes);
      if (knobs.sidebar !== undefined) setSidebarOpen(knobs.sidebar);
      // `?demo=1&project=0` dựng ra trạng thái không-dự-án. Nó nằm trong núm vặn chứ
      // không nằm sau một cú bấm vì đây là trạng thái *đầu tiên* người dùng gặp, và một
      // trạng thái chỉ tới được bằng thao tác là một trạng thái không ai nhớ đi chụp.
      setProjects(demoProjects(knobs.project ?? "p-harness"));
      // Núm vặn cho việc chụp ảnh: cả ba trạng thái dưới đây chỉ tồn tại trong một nhịp
      // bấm chuột, và không có chúng thì cách duy nhất chụp được là sửa mã.
      if (knobs.tab !== undefined && isTab(knobs.tab)) setTab(knobs.tab);
      if (knobs.menu === "project") setProjectMenu(knobs.project ?? "p-harness");
      if (knobs.switching) setSwitching(true);
      if (knobs.state === "skeleton") return; // khung xương đứng yên để nhìn cho kỹ
      const seed = demoSessions(projectKey());
      for (const [id, nodes] of Object.entries(demoParked())) parked.set(id, nodes);
      setSessions(seed);
      setModels(demoModels());
      setModel(demoModels().filter(usableForChat)[0]?.id ?? MODEL_CHUA_BIET);
      setCurrentId(seed[0]?.id ?? "phien-nhap");
      conversation.reset(knobs.state === "empty" ? [] : demoNodes());
      setLoading(false);
      return;
    }
    setProjects(await listProjects());
    const [list, available] = await Promise.all([listSessions(), listModels()]);
    if (list.length > 0) {
      setSessions(list);
      setCurrentId(list[0]!.id);
      void switchTo(list[0]!.id);
    }
    setModels(available);
    // Chỉ chọn trong nhóm trò chuyện được: mặc định rơi vào một mô hình **chỉ** nhúng được
    // là mở ứng dụng lên với một hội thoại chết, và cái tên đó còn không có trong bộ chọn
    // để người dùng thấy mà đổi đi.
    const chat = available.filter(usableForChat);
    // Trong nhóm đó thì ưu tiên mô hình gọi được tool: chọn mặc định một mô hình không gọi
    // được tool là để người dùng gặp một trợ lý không bao giờ đọc được tệp nào mà không
    // hiểu vì sao.
    setModel(chat.find((choice) => choice.tools)?.id ?? chat[0]?.id ?? MODEL_CHUA_BIET);
    setLoading(false);
  });

  /**
   * Câu cảnh báo dưới ô chọn mô hình, hoặc `undefined` khi không có gì phải nói.
   *
   * Hai tình huống im lặng hỏng theo hai kiểu khác nhau, nên chúng có hai câu khác nhau:
   * máy chủ không trả lời được thì chưa chắc đã có gì sai, còn chọn nhầm một mô hình
   * không gọi được tool thì trợ lý sẽ trả lời trôi chảy mà không bao giờ đọc được tệp nào.
   */
  const modelWarning = () => {
    // Không chốt riêng cho demo: trang demo tồn tại đúng để nhìn thấy những trạng thái
    // này mà không cần dựng máy chủ, nên tắt nó ở đó là bỏ mất một nửa công dụng.
    if (models().length === 0) return inTauri() ? "Không hỏi được máy chủ mô hình." : undefined;
    // Máy chủ trả lời tử tế mà không có gì trò chuyện được là một tình huống thứ ba, khác
    // hẳn hai cái kia: không có gì hỏng, chỉ là chưa nạp mô hình nào đúng việc. Im lặng ở
    // đây để lại một cái pill ghi "(chưa hỏi được máy chủ)" — một câu sai.
    if (!models().some(usableForChat)) return "Máy chủ chỉ có mô hình nhúng.";
    const picked = models().find((choice) => choice.id === model());
    if (picked && !picked.tools) return "Mô hình này không gọi được công cụ.";
    return undefined;
  };

  // ⌘/Ctrl+K mở tìm phiên. Bắt ở `window` chứ không ở một ô nhập nào: phím tắt toàn cục
  // phải chạy được kể cả khi tiêu điểm đang ở trong ô soạn tin.
  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.metaKey && !event.ctrlKey) return;
      if (event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      setPaletteOpen(true);
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  async function switchTo(id: string) {
    if (id === currentId()) return;
    parked.set(currentId(), conversation.nodes().slice());
    setCurrentId(id);
    setLoadError(null);
    // Số token thuộc về phiên vừa rời đi. Giữ lại là để chip ngữ cảnh báo 90% trên một
    // phiên trống — một con số sai ở đúng chỗ người dùng dựa vào nó để quyết định có nên
    // bắt đầu phiên mới hay không.
    conversation.clearUsage();

    const cached = parked.get(id);
    if (cached) {
      conversation.reset(cached);
      return;
    }

    conversation.reset([]);
    if (!inTauri()) return;
    setLoadingSession(id);
    try {
      const nodes = nodesFromHistory(await loadSession(id));
      // Người dùng có thể đã chuyển đi trong lúc chờ. Ghi đè lúc đó là hiện bản ghi của
      // phiên họ vừa rời khỏi, dưới cái tên của phiên họ đang mở.
      if (currentId() !== id) return;
      parked.set(id, nodes);
      conversation.reset(nodes);
    } catch (err) {
      if (currentId() === id) setLoadError(String(err));
    } finally {
      if (loadingSession() === id) setLoadingSession(null);
    }
  }

  /**
   * Dọn màn hình sau khi lõi đã chuyển xong dự án.
   *
   * Chạy **sau** khi lõi trả lời chứ không trước: bản ghi và bộ đệm phiên đều thuộc về dự
   * án cũ, và xoá chúng sớm để rồi việc chuyển thất bại là bỏ đi trạng thái của một dự án
   * vẫn đang mở.
   */
  async function adoptProject() {
    parked.clear();
    setLoadError(null);
    conversation.reset([]);

    if (isDemo()) {
      const seed = demoSessions(projectKey());
      setSessions(seed);
      setCurrentId(seed[0]?.id ?? "phien-nhap");
      return;
    }

    // Danh sách phiên **của dự án đang mở**: lõi đã đổi nhánh nên `list_sessions` giờ
    // trả về tập khác. Giao diện không tự lọc — nó không có trường nào để lọc theo, và
    // một bộ lọc đoán ở đây sẽ lệch với cái lõi thật sự đang mang.
    const list = await listSessions();
    setSessions(list);
    const first = list[0];
    setCurrentId(first?.id ?? "phien-nhap");
    if (first) {
      const nodes = nodesFromHistory(await loadSession(first.id));
      parked.set(first.id, nodes);
      conversation.reset(nodes);
    }
  }

  /** Chuyển sang một dự án đã có trong danh sách. */
  async function switchProject(id: string) {
    const target = projects().find((entry) => entry.id === id);
    if (!target || target.isCurrent || switching()) return;
    setSwitching(true);
    try {
      if (isDemo()) {
        await new Promise<void>((resolve) => setTimeout(resolve, 900));
        setProjects((all) =>
          all.map((entry) => ({
            ...entry,
            isCurrent: entry.id === id,
            lastOpenedAt: entry.id === id ? Date.now() : entry.lastOpenedAt,
          })),
        );
      } else {
        await openProject(target.path);
        setProjects(await listProjects());
      }
      await adoptProject();
    } catch (err) {
      setLoadError(`Không chuyển được sang "${target.name}": ${err}`);
    } finally {
      setSwitching(false);
    }
  }

  /**
   * Đóng dự án đang mở và ở lại trong ứng dụng, chỉ trò chuyện.
   *
   * Đi qua đúng đường của `switchProject` — cùng cờ `switching`, cùng `adoptProject` phía
   * sau — vì với màn hình thì đây là một lần đổi dự án nữa, chỉ khác ở chỗ đích đến là
   * "không dự án nào". Lõi cũng tháo nhánh plugin y như vậy.
   *
   * Không hỏi lại trước khi đóng: danh sách không mất dòng nào và mở lại chỉ là hai cú
   * bấm. Một hộp xác nhận cho một việc hoàn tác được chỉ dạy người dùng bấm bừa vào nút
   * đồng ý, và làm hỏng đúng những hộp xác nhận đáng đọc — chỗ xoá phiên chẳng hạn.
   */
  /// Đổi loại dự án đang mở.
  ///
  /// Đi qua đúng đường của `switchProject` — cùng cờ `switching`, cùng `adoptProject` phía
  /// sau — vì lõi tháo và cắm lại cả tầng plugin y như khi đổi dự án. Khác đường thì cờ
  /// bận sẽ không bật, và người dùng bấm tiếp trong lúc tool đang bị gỡ ra.
  async function swapProjectKind(kind: ProjectKind) {
    const open = project();
    if (open === null || switching()) return;
    setSwitching(true);
    setLoadError(null);
    try {
      setProjects(await setProjectKind(open.id, kind));
      await adoptProject();
    } catch (err) {
      setLoadError(`Không đổi được loại dự án: ${err}`);
    } finally {
      setSwitching(false);
    }
  }

  async function closeCurrentProject() {
    if (switching() || !hasProject()) return;
    setSwitching(true);
    try {
      if (isDemo()) {
        await new Promise<void>((resolve) => setTimeout(resolve, 900));
        setProjects((all) => all.map((entry) => ({ ...entry, isCurrent: false })));
      } else {
        // Lõi trả lại cả danh sách sau khi đóng — không dòng nào bị bỏ đi, chỉ không dòng
        // nào còn `isCurrent`.
        setProjects(await closeProject());
      }
      await adoptProject();
    } catch (err) {
      setLoadError(`Không đóng được dự án: ${err}`);
    } finally {
      setSwitching(false);
    }
  }

  /**
   * Mở một thư mục được thả vào cửa sổ làm dự án.
   *
   * Lối vào duy nhất còn lại của `open_project`: mọi lối *có chủ đích* đều đi qua màn hình
   * dự án, nơi có cả loại dự án, clone và hộp thoại chọn thư mục của hệ điều hành. Cú kéo
   * thả sống sót vì nó rẻ hơn mọi lối kia khi cửa sổ Finder đang mở sẵn.
   *
   * Không đoán trước xem đường dẫn là thư mục hay tệp: chỉ lõi mới nhìn được đĩa, và một
   * luật đoán ở đây sẽ từ chối nhầm những thư mục có dấu chấm trong tên.
   */
  async function openFolder(path: string) {
    if (switching()) return;
    setSwitching(true);
    try {
      if (isDemo()) {
        await new Promise<void>((resolve) => setTimeout(resolve, 900));
        const created: Project = {
          id: `demo-${path}`,
          name: folderName(path),
          path,
          lastOpenedAt: Date.now(),
          isCurrent: true,
          kind: "code",
          origin: null,
        };
        setProjects((all) => [
          created,
          ...all.filter((entry) => entry.path !== path).map((entry) => ({ ...entry, isCurrent: false })),
        ]);
      } else {
        await openProject(path);
        setProjects(await listProjects());
      }
      await adoptProject();
    } catch (err) {
      setLoadError(`Không mở được thư mục "${path}": ${err}`);
    } finally {
      setSwitching(false);
    }
  }

  /**
   * Bỏ một dự án **khỏi danh sách**. Không tệp nào trên đĩa bị đụng tới.
   *
   * Bỏ khỏi màn hình trước rồi mới báo cho lõi — ngược với xoá phiên, và ngược có lý do:
   * thao tác này không mất gì cả, nên hỏng thì hàng cũ quay lại là đủ. Bắt người dùng
   * chờ một vòng IPC để bỏ một dòng khỏi danh sách gần đây là trả giá sai chỗ.
   */
  function forgetProject(target: Project) {
    const ok = window.confirm(
      `Bỏ "${target.name}" khỏi danh sách dự án?\n\n` +
        `Thư mục ${target.path} vẫn nguyên trên đĩa — không có tệp nào bị xoá. ` +
        `Mở lại lúc nào cũng được.`,
    );
    if (!ok) return;
    setProjects((all) => all.filter((entry) => entry.id !== target.id));
    if (isDemo()) return;
    void removeProject(target.id).catch(async (err: unknown) => {
      setLoadError(`Không bỏ được "${target.name}" khỏi danh sách: ${err}`);
      setProjects(await listProjects());
    });
  }

  // Thả một thư mục vào cửa sổ là mở nó thành dự án — nhưng **chỉ** khi không có màn hình
  // nào khác đang nhận cú thả. Màn hình dự án và thư viện tài liệu đều gắn cùng cái hook
  // này, và Tauri phát sự kiện cho mọi người nghe: không có chốt ở đây thì một tệp PDF thả
  // vào thư viện cũng bị đem đi mở thành dự án.
  useDragDrop((paths) => {
    if (tab() === "projects" || tab() === "library") return;
    const first = paths[0];
    if (first !== undefined) void openFolder(first);
  });

  async function newSession() {
    // Tên tạm, không đánh số: số thứ tự ở đây tính theo *độ dài danh sách hiện tại*, nên
    // xoá một phiên rồi tạo phiên mới là có hai "Phiên 3". Tên thật đến từ câu hỏi đầu
    // tiên — xem `nameFromFirstMessage`.
    const title = "Phiên mới";
    const created = (await createSession(title)) ?? {
      id: `local-${Date.now()}`,
      title,
      updatedAt: Date.now(),
      preview: null,
    };
    parked.set(currentId(), conversation.nodes().slice());
    setSessions((all) => [created, ...all]);
    setCurrentId(created.id);
    conversation.reset([]);
    setTab("chat");
  }

  /** Đổi tên: sửa trên màn hình trước, rồi báo cho lõi. Ghi hỏng thì chỉ mất cái tên. */
  function rename(id: string) {
    const current = sessions().find((session) => session.id === id);
    const next = window.prompt("Tên mới cho phiên", current?.title ?? "")?.trim();
    if (!next) return;
    setSessions((all) => all.map((s) => (s.id === id ? { ...s, title: next } : s)));
    void renameSession(id, next);
  }

  /**
   * Xoá: hỏi lõi **trước**, rồi mới bỏ khỏi màn hình.
   *
   * Ngược với đổi tên, và cố ý. Xoá không hoàn lại được, nên bỏ khỏi danh sách trước rồi
   * mới biết là lõi từ chối sẽ để người dùng tin một chuyện không xảy ra.
   */
  async function remove(id: string) {
    const current = sessions().find((session) => session.id === id);
    if (!window.confirm(`Xoá phiên "${current?.title ?? id}"? Không hoàn lại được.`)) return;
    try {
      await deleteSession(id);
    } catch (err) {
      setLoadError(`Không xoá được phiên: ${err}`);
      return;
    }
    parked.delete(id);
    const rest = sessions().filter((session) => session.id !== id);
    setSessions(rest);
    if (currentId() === id) {
      const next = rest[0];
      setCurrentId(next?.id ?? "phien-nhap");
      conversation.reset(next ? (parked.get(next.id) ?? []) : []);
    }
  }

  /**
   * Chờ câu hỏi duyệt được trả lời. Chỉ dùng cho lượt giả trong trang demo — lượt thật
   * chặn ở phía Rust, nơi cái hạn giờ đáng tin duy nhất tồn tại.
   */
  function waitForApproval(): Promise<void> {
    return new Promise((resolve) => {
      const timer = setInterval(() => {
        if (!conversation.approval()) {
          clearInterval(timer);
          resolve();
        }
      }, 120);
    });
  }

  function decideApproval(decision: ApprovalDecision) {
    const pending = conversation.approval();
    conversation.clearApproval();
    if (pending) void answerApproval(pending.requestId, decision);
  }

  /**
   * Đặt tên phiên theo câu hỏi đầu tiên, đúng như ChatGPT.
   *
   * Điều kiện là **bản ghi chưa có tin nhắn nào của người dùng**, không phải "tên đang khớp
   * một mẫu nào đó": so tên với `/^Phiên/` sẽ đổi tên cả một phiên mà người dùng cố ý đặt
   * tên là "Phiên thử nghiệm". Còn "chưa ai hỏi gì" thì đúng một lần xảy ra trong đời một
   * phiên, và đó là lần duy nhất được phép ghi đè.
   *
   * Gọi **trước** `addUser` vì sau đó bản ghi đã có tin nhắn ấy rồi.
   */
  function nameFromFirstMessage(text: string) {
    if (conversation.nodes().some((node) => node.kind === "user")) return;
    const title = titleFromMessage(text);
    if (title === "") return;
    const id = currentId();
    if (!sessions().some((session) => session.id === id)) return;
    // Đổi trên màn hình trước rồi báo cho lõi, đúng như `rename`: ghi hỏng thì chỉ mất
    // cái tên, và một hộp thoại lỗi ngay lúc gửi câu hỏi đầu tiên là cắt ngang sai chỗ.
    setSessions((all) => all.map((s) => (s.id === id ? { ...s, title } : s)));
    void renameSession(id, title);
  }

  async function send(text: string) {
    const trimmed = text.trim();
    if (trimmed === "") return;
    // Lượt trước chưa xong: giữ lại, đừng nuốt. Trả về ở đây mà không giữ gì là đúng cái
    // cách cũ làm mất một câu vừa gõ xong.
    if (conversation.busy()) {
      setQueued(trimmed);
      setDraft("");
      return;
    }

    nameFromFirstMessage(trimmed);
    conversation.addUser(trimmed);
    setDraft("");
    conversation.setBusy(true);
    setTab("chat");

    // Chốt phiên **trước** khi gửi, và dùng nó cho cả `finally`: `currentId()` có thể đã
    // đổi khi lượt kết thúc.
    const cuaLuot = currentId();

    try {
      // Sự kiện của lượt này chỉ được ghi vào **phiên đã gửi nó**. Người dùng đổi phiên
      // giữa lượt là chuyện thường, và không có chốt này thì token cùng thẻ tool của lượt
      // cũ rơi thẳng vào bản ghi của phiên vừa mở — một bản ghi bịa, và nó được lưu lại.
      //
      // Lượt cũ vẫn chạy tiếp tới cùng ở phía lõi và vẫn vào sổ; quay lại phiên đó sẽ thấy
      // đủ. Bỏ sự kiện ở đây chỉ là bỏ phần vẽ trực tiếp, không bỏ câu trả lời.
      const applyIfCurrent = (event: AgentEvent) => {
        if (currentId() !== cuaLuot) return;
        conversation.applyEvent(event);
      };
      // Chốt phạm vi cùng lúc với phiên: người dùng đổi mức trong lúc lượt đang chạy
      // thì lượt này vẫn chạy đúng mức nó đã được gửi đi, và mức mới thuộc về lượt sau.
      const quyen = scope();
      if (isDemo() || !inTauri()) {
        await runDemoTurn(trimmed, quyen, applyIfCurrent, waitForApproval);
      } else {
        await sendMessage(cuaLuot, trimmed, quyen, applyIfCurrent);
      }
    } catch (err) {
      conversation.applyEvent({ kind: "error", message: String(err) });
    } finally {
      // Bất kể lượt kết thúc thế nào, câu hỏi duyệt còn treo phải đóng lại — và đóng
      // như một lần từ chối, vì không còn ai bên kia để nhận câu trả lời nữa.
      if (conversation.approval()) decideApproval("rejected");
      conversation.finishTurn();
      // Bản chụp đã park của phiên vừa gửi cũng phải hết "đang chảy", nếu không quay lại
      // nó sẽ thấy một con trỏ nhấp nháy vĩnh viễn trên một lượt đã xong từ lâu.
      const chup = parked.get(cuaLuot);
      if (chup) {
        parked.set(
          cuaLuot,
          chup.map((node) =>
            node.kind === "assistant" && node.streaming ? { ...node, streaming: false } : node,
          ),
        );
      }

      // Câu đang chờ thuộc về **phiên đã nhận nó**. Người dùng đổi phiên giữa lượt thì gửi
      // nó vào phiên mới là gửi một câu hỏi vào một ngữ cảnh nó không nói về — nên nó rơi
      // về ô soạn tin, còn nguyên chữ, để họ quyết định. Chỉ rơi khi bản nháp đang trống:
      // ghi đè lên thứ họ vừa gõ là đổi một phiền toái lấy một mất mát.
      const cho = queued();
      if (cho !== "") {
        setQueued("");
        if (currentId() === cuaLuot) void send(cho);
        else setDraft((hien) => (hien.trim() === "" ? cho : hien));
      }
    }
  }

  /**
   * Chạy một lệnh `/` từ ô soạn tin.
   *
   * Mỗi nhánh ở đây phải trỏ vào một hành động **đã tồn tại**, không được là một đường đi
   * thứ hai tự viết lấy: hai đường tới cùng một việc là hai chỗ để chúng lệch nhau, và
   * người dùng gặp cái lệch ấy dưới dạng "bấm menu thì được, gõ lệnh thì không".
   */
  function runCommand(name: string) {
    switch (name) {
      case "moi":
        void newSession();
        break;
      case "tim":
        setPaletteOpen(true);
        break;
      case "duan":
        setTab("projects");
        break;
      case "thaydoi":
        setChangesPanelOpen(true);
        break;
      case "taplieu":
        setTab("library");
        break;
      case "mohinh":
        setSettingsPage("provider");
        setTab("settings");
        break;
      case "mcp":
        setSettingsPage("mcp");
        setTab("settings");
        break;
      case "quyen":
        setSettingsPage("quyen");
        setTab("settings");
        break;
      case "phimtat":
        setSettingsPage("phim-tat");
        setTab("settings");
        break;
      case "caidat":
        setSettingsPage("chung");
        setTab("settings");
        break;
    }
  }

  /** Cuộn tới node mà một tệp trong bảng thay đổi trỏ về. */
  function reveal(nodeId: string) {
    setTab("chat");
    queueMicrotask(() => {
      const el = document.getElementById(`node-${nodeId}`);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      // Nháy viền một nhịp: sau một cú cuộn mượt, người dùng cần biết *cái nào* vừa được
      // đưa tới, chứ không chỉ biết là màn hình đã dịch chuyển.
      //
      // `outline-style` **không** animate được, nên phải đặt sẵn một đường viền trong suốt
      // trước khi chạy: không đặt thì cả hiệu ứng này chạy trên `outline-style: none` và
      // không nháy gì cả — hỏng trong im lặng, đúng kiểu không ai phát hiện ra.
      el.style.outline = "2px solid transparent";
      el.style.outlineOffset = "2px";
      const clear = () => {
        el.style.outline = "";
        el.style.outlineOffset = "";
      };
      const flash = el.animate?.(
        [{ outlineColor: "var(--accent)" }, { outlineColor: "transparent" }],
        { duration: 900, easing: "ease-out" },
      );
      if (flash) flash.onfinish = clear;
      else clear();
    });
  }

  /** Bản ghi chưa có gì — trạng thái quyết định ô soạn tin ngồi giữa hay ngồi đáy. */
  const chatEmpty = () => conversation.nodes().length === 0;
  /**
   * Có hiện mấy câu gợi ý không.
   *
   * Chỉ khi bản ghi trống **và** không có gì khác đang chiếm chỗ đó: một danh sách câu hỏi
   * mẫu nằm dưới dòng "Đang nạp bản ghi…" mời người dùng bắt đầu một việc mà nửa giây nữa
   * sẽ bị một bản ghi cũ đè lên.
   */
  const showPrompts = () => chatEmpty() && loadingSession() === null && loadError() === null;

  const title = () =>
    tab() === "chat"
      ? (sessions().find((session) => session.id === currentId())?.title ?? "Phiên làm việc")
      : TAB_TITLE[tab()];

  return (
    <TranscriptActionsProvider
      value={{
        resend: conversation.busy() ? null : (text) => void send(text),
        remove: conversation.removeNode,
        // Không còn màn hình nào đọc tệp, nên đường dẫn trong bản ghi hiện dưới dạng chữ
        // chứ không dưới dạng nút. Một đường dẫn trông như nút bấm mà bấm không ra gì tệ
        // hơn hẳn một đường dẫn trông như chữ.
        openFile: null,
      }}
    >
      <div class="flex h-full bg-bg">
        {/* Khu làm việc gói trong một lớp riêng chỉ vì một lý do: khi cài đặt mở, cả lớp
            này phải thành `inert`. Màn hình cài đặt phủ kín cửa sổ nên mắt không thấy gì
            bên dưới, nhưng Tab thì vẫn đi xuống được — và một người dùng bàn phím sẽ đi
            lạc vào một ô soạn tin họ không nhìn thấy. `inert` gỡ cả nhánh khỏi vòng Tab
            lẫn khỏi cây trợ năng, đúng thứ `aria-hidden` một mình không làm được. */}
        <div
          class="flex min-w-0 flex-1"
          ref={(el) => {
            // Đặt bằng `toggleAttribute` chứ không bằng một prop JSX: `inert` chưa có
            // trong kiểu JSX của Solid, và một `as any` để lách kiểu ở đây sẽ tắt luôn
            // việc kiểm kiểu cho mọi prop khác của thẻ này.
            createEffect(() => el.toggleAttribute("inert", tab() === "settings"));
          }}
        >
        <Show when={sidebarOpen()}>
          <Sidebar
            sessions={sessions()}
            currentId={currentId()}
            loading={loading()}
            view={tab()}
            mcpCount={mcpConnected()}
            subtitle={preview}
            disabled={switching()}
            projectsSlot={() => (
              <ProjectSwitcher
                projects={projects()}
                current={project()}
                switching={switching()}
                menuFor={projectMenu()}
                onMenuChange={setProjectMenu}
                // Mục con của dự án đang mở. Dựng ở đây chứ không trong thanh bên vì đây
                // là chỗ duy nhất biết cả loại dự án lẫn màn hình đang mở.
                subItems={projectTabs(project()?.kind).map((item) => ({
                  id: item.id,
                  label: item.label,
                  icon: item.icon,
                  badge: item.id === "diff" ? files().length : 0,
                  active: tab() === item.id,
                  onSelect: () => setTab(item.id),
                }))}
                onPick={(id) => void switchProject(id)}
                onSeeAll={() => setTab("projects")}
                onForget={forgetProject}
                onClose={() => void closeCurrentProject()}
                onSwapKind={(kind) => void swapProjectKind(kind)}
              />
            )}
            onSelect={(id) => void switchTo(id)}
            onCreate={() => void newSession()}
            onRename={rename}
            onDelete={remove}
            onGo={(view) => {
              if (view === "settings") setSettingsPage("chung");
              setTab(view);
            }}
            onOpenMcp={() => {
              setSettingsPage("mcp");
              setTab("settings");
            }}
            onCollapse={() => setSidebarOpen(false)}
          />
        </Show>

        <main class="flex min-w-0 flex-1 flex-col">
          <WorkspaceHeader
            title={title()}
            scope={project()?.name}
            busy={conversation.busy() || switching()}
            busyLabel={switching() ? "đang chuyển dự án…" : undefined}
            sidebarOpen={sidebarOpen()}
            onOpenSidebar={() => setSidebarOpen(true)}
            changesPanelOpen={tab() === "chat" ? changesPanelOpen() : undefined}
            changeCount={files().length}
            // Không dự án thì không tool nào chạm được tới đĩa, nên bảng thay đổi vĩnh
            // viễn trống. Một công tắc mở ra một bảng rỗng là một lời hứa suông.
            onToggleChangesPanel={
              tab() === "chat" && hasProject()
                ? () => setChangesPanelOpen(!changesPanelOpen())
                : undefined
            }
          />

          <div class="flex min-h-0 flex-1">
            <div class="flex min-w-0 flex-1 flex-col">
              <Switch>
                <Match when={tab() === "chat"}>
                  {/* Bản ghi trống thì cả khối "câu hỏi + ô soạn tin" trôi vào **giữa màn
                      hình theo chiều dọc**, đúng như ChatGPT: một trạng thái rỗng dán ở đầu
                      trang với ô nhập ở tận đáy bắt mắt đi hết chiều cao cửa sổ để nối hai
                      thứ vốn thuộc về nhau. Có hội thoại rồi thì ô soạn tin về đáy như thường.

                      Ô soạn tin là **một** thể hiện duy nhất qua cả hai bố cục — chỉ lớp CSS
                      của khung ngoài đổi. Dựng lại nó ngay lúc câu hỏi đầu tiên được gửi sẽ
                      cướp tiêu điểm bàn phím đúng vào nhịp người dùng định gõ tiếp. */}
                  <div
                    class="flex min-h-0 flex-1 flex-col"
                    classList={{ "justify-center": chatEmpty() }}
                  >
                    <Show
                      when={!chatEmpty()}
                      fallback={
                        <div class="min-h-0 shrink overflow-y-auto py-lg">
                          <Switch
                            fallback={
                              <EmptyLead
                                kind={project()?.kind ?? null}
                                onOpenProject={() => setTab("projects")}
                              />
                            }
                          >
                            <Match when={loadingSession()}>
                              <p
                                class="m-0 text-center text-sm text-muted"
                                role="status"
                                aria-live="polite"
                              >
                                Đang nạp bản ghi…
                              </p>
                            </Match>
                            <Match when={loadError()}>
                              {(message) => (
                                <p
                                  class="mx-auto max-w-(--reading-measure) rounded-panel bg-danger-soft px-md py-sm text-sm text-danger"
                                  role="alert"
                                >
                                  {message()}
                                </p>
                              )}
                            </Match>
                          </Switch>
                        </div>
                      }
                    >
                      <Transcript nodes={conversation.nodes()} />
                    </Show>

                    <Composer
                      value={draft()}
                      onChange={setDraft}
                      onSubmit={() => void send(draft())}
                      disabled={switching()}
                      busy={conversation.busy()}
                      queued={queued()}
                      onUnqueue={() => setQueued("")}
                      onStop={() => void cancelTurn(currentId())}
                      onCommand={runCommand}
                      usage={conversation.usage()}
                      model={model()}
                      models={models()}
                      onPickModel={setModel}
                      onManageProviders={() => {
                        setSettingsPage("provider");
                        setTab("settings");
                      }}
                      modelWarning={modelWarning()}
                      scope={scope()}
                      onPickScope={setScope}
                      hasProject={hasProject()}
                      projectName={project()?.name}
                      projectKind={project()?.kind}
                      mcpConnected={mcpConnected()}
                      moreBelow={showPrompts()}
                    />

                    {/* Gợi ý nằm **dưới** ô soạn tin: câu hỏi lớn phải chạm thẳng vào chỗ
                        trả lời nó, còn mấy câu bấm được là lối tắt, và một lối tắt chen vào
                        giữa hai thứ ấy đẩy chúng ra xa nhau. */}
                    <Show when={showPrompts()}>
                      <div class="shrink-0 px-(--page-pad-x) pb-(--page-pad-y)">
                        <PromptChips
                          disabled={conversation.busy()}
                          kind={project()?.kind ?? null}
                          onPick={(text) => void send(text)}
                        />
                      </div>
                    </Show>
                  </div>
                </Match>

                <Match when={tab() === "diff"}>
                  <ChangesBoard files={files()} onReveal={reveal} />
                </Match>

                <Match when={tab() === "projects"}>
                  <ProjectsView
                    projects={projects()}
                    switching={switching()}
                    error={loadError()}
                    onOpen={(picked) => void switchProject(picked.id)}
                    onForget={forgetProject}
                    onCreated={async () => {
                      setProjects(await listProjects());
                      await adoptProject();
                    }}
                  />
                </Match>

                <Match when={tab() === "library"}>
                  {/* `resetKey` là đường dẫn dự án: đổi dự án là nạp lại thư viện từ đầu.
                      Dùng id thì một dự án bị bỏ rồi thêm lại sẽ mang id mới cho cùng một
                      thư viện, và màn hình nạp lại một thứ không đổi. */}
                  <DocsView resetKey={project()?.path ?? ""} name={project()?.name} />
                </Match>

              </Switch>
            </div>

            <Show when={tab() === "chat" && changesPanelOpen() && hasProject()}>
              <ChangesPanel
                files={files()}
                onReveal={reveal}
                onClose={() => setChangesPanelOpen(false)}
              />
            </Show>
          </div>
        </main>
        </div>

        {/* Cài đặt là một chế độ **chiếm trọn cửa sổ**, nên nó nằm ngoài `<main>` chứ
            không nằm trong `<Switch>` của khu làm việc: thanh bên và ô soạn tin không có
            việc gì ở đó, và để chúng lấp ló bên cạnh là mời người dùng bấm nhầm vào một
            phiên trong lúc đang sửa một khoá API. Cây bên dưới vẫn được giữ nguyên chứ
            không tháo đi — quay về hội thoại thì bản ghi vẫn ở đúng chỗ đang đọc.

            `z-30`, thấp hơn hộp thoại (`z-50`): hộp thoại duyệt và bảng chọn phiên vẫn
            phải nổi lên trên cài đặt. */}
        <Show when={tab() === "settings"}>
          <SettingsView
            page={settingsPage()}
            onPage={setSettingsPage}
            onClose={() => setTab("chat")}
          />
        </Show>

        <Show when={conversation.approval()}>
          {(request) => <ApprovalDialog request={request()} onDecide={decideApproval} />}
        </Show>

        <Show when={paletteOpen()}>
          <SessionPalette
            sessions={sessions()}
            currentId={currentId()}
            onPick={(id) => {
              switchTo(id);
              setTab("chat");
              setPaletteOpen(false);
            }}
            onClose={() => setPaletteOpen(false)}
          />
        </Show>
      </div>
    </TranscriptActionsProvider>
  );
}

const TAB_TITLE: Record<TabId, string> = {
  chat: "Hội thoại",
  diff: "Thay đổi trong phiên",
  library: "Thư viện tài liệu",
  projects: "Dự án",
  settings: "Cài đặt",
};

/** Núm `?tab=` của trang demo đến từ URL, nên nó là một chuỗi bất kỳ cho tới khi kiểm. */
const isTab = (raw: string): raw is TabId => Object.hasOwn(TAB_TITLE, raw);
