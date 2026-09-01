import { createMemo, createSignal, Match, onCleanup, onMount, Show, Switch } from "solid-js";
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
  demoFile,
  demoFilePaths,
  demoKnobs,
  demoModels,
  demoNodes,
  demoParked,
  demoProjects,
  demoRoot,
  demoSessions,
  demoTree,
  isDemo,
  runDemoTurn,
} from "./lib/demo";
import { changesPanelOpen, setChangesPanelOpen, setDisplayMode, setSessionPanelOpen, sessionPanelOpen } from "./lib/prefs";
import {
  absolutePath,
  displayPath,
  folderName,
  listProjects,
  listTree,
  openProject,
  readFile,
  removeProject,
} from "./lib/projects";
import type {
  ApprovalDecision,
  ConversationNode,
  ModelChoice,
  Project,
  SessionSummary,
  TreeEntry,
} from "./lib/protocol";
import { TranscriptActionsProvider } from "./lib/transcriptActions";
import { useDragDrop } from "./hooks/useDragDrop";
import ApprovalDialog from "./components/ApprovalDialog";
import ChangesPanel from "./components/ChangesPanel";
import CodeBrowser from "./components/CodeBrowser";
import Composer, { type ToolScope } from "./components/Composer";
import EmptyState from "./components/EmptyState";
import FilePalette from "./components/FilePalette";
import Icon from "./components/Icon";
import { IconButton } from "./components/primitives";
import OpenProjectDialog from "./components/OpenProjectDialog";
import ProjectSwitcher from "./components/ProjectSwitcher";
import Rail, { type TabId } from "./components/Rail";
import SessionPalette from "./components/SessionPalette";
import SessionPanel from "./components/SessionPanel";
import SettingsView from "./components/SettingsView";
import Transcript from "./components/Transcript";
import WorkspaceHeader from "./components/WorkspaceHeader";

// Nạp sổ đăng ký renderer. Import vì hiệu ứng phụ là cố ý: đây là chỗ *duy nhất* biết
// danh sách renderer, nên thêm một loại node mới không kéo theo sửa đổi ở nơi nào khác.
import "./components/nodes";

/** Mô hình dùng khi chưa hỏi được máy chủ. Chỉ để ô chọn không trống. */
const MODEL_CHUA_BIET = "(chưa hỏi được máy chủ)";

/**
 * Vỏ ứng dụng: rail biểu tượng, danh sách phiên, khu làm việc, và một bảng thay đổi
 * mở/đóng được ở bên phải.
 *
 * Trạng thái hội thoại nằm trong một store riêng cho từng phiên và được nhớ lại khi
 * quay về — chuyển phiên rồi mất chỗ đang đọc là cách nhanh nhất làm người ta ngại
 * chuyển phiên.
 */
export default function App() {
  const conversation = createConversation();
  const [sessions, setSessions] = createSignal<SessionSummary[]>([]);
  const [currentId, setCurrentId] = createSignal("phien-nhap");
  const [draft, setDraft] = createSignal("");
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [tab, setTab] = createSignal<TabId>("chat");
  const [loading, setLoading] = createSignal(true);
  const [models, setModels] = createSignal<ModelChoice[]>([]);
  const [model, setModel] = createSignal(MODEL_CHUA_BIET);
  // Phiên đang được nạp lại từ sổ. Giữ id chứ không giữ boolean: chuyển phiên nhanh hai
  // lần thì kết quả về sau của phiên cũ không được ghi đè lên phiên mới.
  const [loadingSession, setLoadingSession] = createSignal<string | null>(null);
  const [loadError, setLoadError] = createSignal<string | null>(null);
  const [scope, setScope] = createSignal<ToolScope>("write");

  const [projects, setProjects] = createSignal<Project[]>([]);
  // Đổi dự án là lõi tháo và cắm lại cả một nhánh plugin. Trong lúc đó mọi thứ trên màn
  // hình còn nói về dự án cũ, nên cờ này khoá thao tác thay vì chỉ hiện một cái chấm quay.
  const [switching, setSwitching] = createSignal(false);
  const [projectMenuOpen, setProjectMenuOpen] = createSignal(false);
  const [openDialog, setOpenDialog] = createSignal(false);
  const [dialogError, setDialogError] = createSignal<string | null>(null);
  const [openFile, setOpenFile] = createSignal<{ path: string; line?: number } | null>(null);
  const [filePaletteOpen, setFilePaletteOpen] = createSignal(false);

  const project = () => projects().find((entry) => entry.isCurrent) ?? null;
  const projectKey = () => project()?.id ?? "khong-co-du-an";

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
      if (knobs.mode) setDisplayMode(knobs.mode);
      if (knobs.changes !== undefined) setChangesPanelOpen(knobs.changes);
      setProjects(demoProjects());
      // Núm vặn cho việc chụp ảnh: cả ba trạng thái dưới đây chỉ tồn tại trong một nhịp
      // bấm chuột, và không có chúng thì cách duy nhất chụp được là sửa mã.
      if (knobs.tab === "code" || knobs.tab === "diff" || knobs.tab === "chat") setTab(knobs.tab);
      if (knobs.menu === "project") setProjectMenuOpen(true);
      if (knobs.switching) setSwitching(true);
      if (knobs.file) setOpenFile({ path: `${demoRoot("p-harness")}/${knobs.file}` });
      if (knobs.state === "skeleton") return; // khung xương đứng yên để nhìn cho kỹ
      const seed = demoSessions("p-harness");
      for (const [id, nodes] of Object.entries(demoParked())) parked.set(id, nodes);
      setSessions(seed);
      setModels(demoModels());
      setModel(demoModels()[0]?.id ?? MODEL_CHUA_BIET);
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
    // Ưu tiên một mô hình gọi được tool: chọn mặc định một mô hình không gọi được tool
    // là để người dùng gặp một trợ lý không bao giờ đọc được tệp nào mà không hiểu vì sao.
    setModel(
      available.find((choice) => choice.tools)?.id ?? available[0]?.id ?? MODEL_CHUA_BIET,
    );
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
    const picked = models().find((choice) => choice.id === model());
    if (picked && !picked.tools) return "Mô hình này không gọi được công cụ.";
    return undefined;
  };

  // ⌘/Ctrl+K mở tìm phiên. Bắt ở `window` chứ không ở một ô nhập nào: phím tắt toàn cục
  // phải chạy được kể cả khi tiêu điểm đang ở trong ô soạn tin.
  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.metaKey && !event.ctrlKey) return;
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        setPaletteOpen(true);
      } else if (key === "p") {
        // ⌘P mở tìm tệp và chuyển sang tab Mã nguồn luôn: chọn xong mà vẫn đứng ở tab cũ
        // thì cú bấm không dẫn tới đâu, và người dùng phải tự đoán ra tệp đã mở ở chỗ nào.
        event.preventDefault();
        setTab("code");
        setFilePaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  async function switchTo(id: string) {
    if (id === currentId()) return;
    parked.set(currentId(), conversation.nodes().slice());
    setCurrentId(id);
    setLoadError(null);

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
   * Chạy **sau** khi lõi trả lời chứ không trước: bản ghi, bộ đệm phiên và tệp đang mở
   * đều thuộc về dự án cũ, và xoá chúng sớm để rồi việc chuyển thất bại là bỏ đi trạng
   * thái của một dự án vẫn đang mở.
   */
  async function adoptProject() {
    parked.clear();
    setOpenFile(null);
    setLoadError(null);
    fileIndex = null;
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
   * Mở một thư mục làm dự án: từ hộp thoại, hoặc từ một thư mục kéo vào cửa sổ.
   *
   * Lỗi đi về hai chỗ khác nhau tuỳ lối vào. Hộp thoại còn đang mở thì lỗi nằm ngay dưới
   * ô nhập, cạnh đường dẫn vừa gõ; còn với cú kéo thả thì không có ô nhập nào để đứng
   * cạnh, nên nó lên chỗ báo lỗi chung.
   */
  async function openFolder(path: string) {
    if (switching()) return;
    setSwitching(true);
    setDialogError(null);
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
      setOpenDialog(false);
    } catch (err) {
      if (openDialog()) setDialogError(String(err));
      else setLoadError(`Không mở được thư mục "${path}": ${err}`);
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

  /**
   * Mở một tệp trong tab Mã nguồn, ở đúng dòng nếu chỗ gọi biết.
   *
   * Chuẩn hoá đường dẫn ở đây và chỉ ở đây. Cây tệp đưa vào đường dẫn tuyệt đối, còn thẻ
   * tool và bảng thay đổi đưa vào đường dẫn tương đối với gốc dự án — cả hai đều đúng
   * với nguồn của chúng, và `read_file` chỉ nhận một trong hai.
   */
  function showFile(rawPath: string, line?: number) {
    const path = absolutePath(project()?.path ?? null, rawPath);
    setOpenFile(line === undefined ? { path } : { path, line });
    setTab("code");
    setFilePaletteOpen(false);
  }

  const loadTree = (path?: string): Promise<TreeEntry[]> =>
    isDemo() ? demoTree(projectKey(), path, 1) : listTree(path, 1);

  const loadFile = (path: string) => (isDemo() ? demoFile(path) : readFile(path));

  // Bảng ⌘P cần *mọi* tên tệp, thứ cây nạp lười cố ý không có. Xin một lần rồi giữ:
  // trả giá đúng một lần cho mỗi dự án, ở lúc người dùng đã nói rằng họ cần nó.
  let fileIndex: { key: string; paths: string[] } | null = null;

  async function loadFilePaths(): Promise<string[]> {
    const key = projectKey();
    if (fileIndex?.key === key) return fileIndex.paths;
    const paths = isDemo() ? await demoFilePaths(key) : flattenTree(await listTree(undefined, 8));
    fileIndex = { key, paths };
    return paths;
  }

  // Thả một thư mục vào cửa sổ là mở nó. Không đoán trước xem đường dẫn là thư mục hay
  // tệp: chỉ lõi mới nhìn được đĩa, và một luật đoán ở đây sẽ từ chối nhầm những thư mục
  // có dấu chấm trong tên.
  useDragDrop((paths) => {
    const first = paths[0];
    if (first !== undefined) void openFolder(first);
  });

  async function newSession() {
    const title = `Phiên ${sessions().length + 1}`;
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

  async function send(text: string) {
    const trimmed = text.trim();
    if (conversation.busy() || trimmed === "") return;

    conversation.addUser(trimmed);
    setDraft("");
    conversation.setBusy(true);
    setTab("chat");

    try {
      if (isDemo() || !inTauri()) {
        await runDemoTurn(trimmed, conversation.applyEvent, waitForApproval);
      } else {
        await sendMessage(currentId(), trimmed, conversation.applyEvent);
      }
    } catch (err) {
      conversation.applyEvent({ kind: "error", message: String(err) });
    } finally {
      // Bất kể lượt kết thúc thế nào, câu hỏi duyệt còn treo phải đóng lại — và đóng
      // như một lần từ chối, vì không còn ai bên kia để nhận câu trả lời nữa.
      if (conversation.approval()) decideApproval("rejected");
      conversation.finishTurn();
    }
  }

  /** Cuộn tới node mà một tệp trong bảng thay đổi trỏ về. */
  function reveal(nodeId: string) {
    setTab("chat");
    queueMicrotask(() => {
      const el = document.getElementById(`node-${nodeId}`);
      el?.scrollIntoView({ behavior: "smooth", block: "center" });
      // Nháy viền một nhịp: sau một cú cuộn mượt, người dùng cần biết *cái nào* vừa được
      // đưa tới, chứ không chỉ biết là màn hình đã dịch chuyển.
      el?.animate?.(
        [{ outlineColor: "var(--accent)" }, { outlineColor: "transparent" }],
        { duration: 900, easing: "ease-out" },
      );
    });
  }

  const title = () =>
    sessions().find((session) => session.id === currentId())?.title ?? "Phiên làm việc";

  return (
    <TranscriptActionsProvider
      value={{
        resend: conversation.busy() ? null : (text) => void send(text),
        remove: conversation.removeNode,
        openFile: showFile,
      }}
    >
      <div class="flex h-full bg-bg">
        <Rail active={tab()} onSelect={setTab} disabled={switching()} />

        <Show when={tab() === "chat" && sessionPanelOpen()}>
          <SessionPanel
            sessions={sessions()}
            currentId={currentId()}
            loading={loading()}
            subtitle={preview}
            disabled={switching()}
            projectSlot={() => (
              <ProjectSwitcher
                projects={projects()}
                current={project()}
                switching={switching()}
                open={projectMenuOpen()}
                onOpenChange={setProjectMenuOpen}
                onPick={(id) => void switchProject(id)}
                onOpenFolder={() => {
                  setDialogError(null);
                  setOpenDialog(true);
                }}
                onForget={forgetProject}
              />
            )}
            onSelect={(id) => void switchTo(id)}
            onCreate={() => void newSession()}
            onRename={rename}
            onDelete={remove}
            onCollapse={() => setSessionPanelOpen(false)}
          />
        </Show>

        <main class="flex min-w-0 flex-1 flex-col">
          <WorkspaceHeader
            title={tab() === "chat" ? title() : TAB_TITLE[tab()]}
            model={tab() === "chat" ? model() : undefined}
            scope={
              tab() === "chat"
                ? `${files().length} tệp đã đổi`
                : tab() === "code"
                  ? (openFile() === null
                      ? project()?.path
                      : displayPath(project()?.path ?? null, openFile()!.path))
                  : undefined
            }
            busy={conversation.busy() || switching()}
            busyLabel={switching() ? "đang chuyển dự án…" : undefined}
            sessionPanelOpen={sessionPanelOpen()}
            changesPanelOpen={changesPanelOpen()}
            changeCount={files().length}
            onToggleSessionPanel={() => {
              setTab("chat");
              setSessionPanelOpen(!sessionPanelOpen());
            }}
            onToggleChangesPanel={() => setChangesPanelOpen(!changesPanelOpen())}
            onSearch={() => setPaletteOpen(true)}
          />

          <div class="flex min-h-0 flex-1">
            <div class="flex min-w-0 flex-1 flex-col">
              <Switch>
                <Match when={tab() === "chat"}>
                  <Transcript
                    nodes={conversation.nodes()}
                    empty={
                      <Switch
                        fallback={
                          <EmptyState
                            disabled={conversation.busy()}
                            onPick={(text) => void send(text)}
                          />
                        }
                      >
                        <Match when={loadingSession()}>
                          <p
                            class="m-auto text-sm text-muted"
                            role="status"
                            aria-live="polite"
                          >
                            Đang nạp bản ghi…
                          </p>
                        </Match>
                        <Match when={loadError()}>
                          {(message) => (
                            <p
                              class="m-auto max-w-(--reading-measure) rounded-panel bg-danger-soft px-md py-sm text-sm text-danger"
                              role="alert"
                            >
                              {message()}
                            </p>
                          )}
                        </Match>
                      </Switch>
                    }
                  />
                  <Composer
                    value={draft()}
                    onChange={setDraft}
                    onSubmit={() => void send(draft())}
                    disabled={conversation.busy() || switching()}
                    busy={conversation.busy()}
                    onStop={() => void cancelTurn(currentId())}
                    model={model()}
                    models={models().map((choice) => choice.id)}
                    onPickModel={setModel}
                    modelWarning={modelWarning()}
                    scope={scope()}
                    onPickScope={setScope}
                  />
                </Match>

                <Match when={tab() === "diff"}>
                  <ChangesBoard files={files()} onReveal={reveal} onOpenFile={showFile} />
                </Match>

                <Match when={tab() === "code"}>
                  <CodeBrowser
                    projectId={projectKey()}
                    projectName={project()?.name ?? "Chưa mở dự án"}
                    root={project()?.path ?? null}
                    loadTree={loadTree}
                    loadFile={loadFile}
                    open={openFile()}
                    onOpen={showFile}
                    onFind={() => setFilePaletteOpen(true)}
                  />
                </Match>

                <Match when={tab() === "terminal"}>
                  <NotBuilt
                    what="Terminal"
                    why="Lệnh chạy qua tool bash và hiện trong bản ghi hội thoại; một PTY đứng riêng còn nằm trong lộ trình."
                  />
                </Match>

                <Match when={tab() === "settings"}>
                  <SettingsView />
                </Match>
              </Switch>
            </div>

            <Show when={tab() === "chat" && changesPanelOpen()}>
              <ChangesPanel
                files={files()}
                onReveal={reveal}
                onOpenFile={showFile}
                onClose={() => setChangesPanelOpen(false)}
              />
            </Show>
          </div>
        </main>

        <Show when={conversation.approval()}>
          {(request) => <ApprovalDialog request={request()} onDecide={decideApproval} />}
        </Show>

        <Show when={openDialog()}>
          <OpenProjectDialog
            busy={switching()}
            error={dialogError()}
            onSubmit={(path) => void openFolder(path)}
            onClose={() => setOpenDialog(false)}
          />
        </Show>

        <Show when={filePaletteOpen()}>
          <FilePalette
            load={loadFilePaths}
            root={project()?.path ?? null}
            onPick={(path) => showFile(path)}
            onClose={() => setFilePaletteOpen(false)}
          />
        </Show>

        <Show when={paletteOpen()}>
          <SessionPalette
            sessions={sessions()}
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
  code: "Mã nguồn",
  terminal: "Terminal",
  settings: "Cài đặt",
};

/** Mọi đường dẫn tệp trong một cây đã nạp — đầu vào của bảng tìm tệp. */
function flattenTree(entries: TreeEntry[]): string[] {
  const out: string[] = [];
  const walk = (list: TreeEntry[]) => {
    for (const entry of list) {
      if (entry.isDir) walk(entry.children ?? []);
      else out.push(entry.path);
    }
  };
  walk(entries);
  return out;
}

/** Bảng thay đổi ở dạng trang đầy — cùng dữ liệu với cột phải, chỉ khác chỗ ngồi. */
function ChangesBoard(props: {
  files: ReturnType<typeof changedFiles>;
  onReveal: (nodeId: string) => void;
  onOpenFile: (path: string) => void;
}) {
  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-(--reading-measure) flex-col gap-sm">
        <Show
          when={props.files.length > 0}
          fallback={<p class="text-sm text-faint">Phiên này chưa đụng vào tệp nào.</p>}
        >
          {props.files.map((file) => (
            <div class="flex items-center gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) transition-colors duration-[var(--dur-fast)] hover:border-accent">
              <button
                type="button"
                onClick={() => props.onReveal(file.nodeId)}
                class="flex min-w-0 flex-1 items-center gap-md text-left"
              >
                <span class="text-muted">
                  <Icon name="diff" size={16} />
                </span>
                <span class="min-w-0 flex-1 truncate font-mono text-xs text-text" title={file.path}>
                  {file.path}
                </span>
                <span class="shrink-0 text-2xs tabular-nums">
                  <span class="text-success">+{file.added}</span>{" "}
                  <span class="text-danger">−{file.removed}</span>
                </span>
              </button>
              <IconButton
                icon="code"
                label={`Mở ${file.path} trong Mã nguồn`}
                size="sm"
                onClick={() => props.onOpenFile(file.path)}
              />
            </div>
          ))}
        </Show>
      </div>
    </div>
  );
}

function NotBuilt(props: { what: string; why: string }) {
  return (
    <div class="grid min-h-0 flex-1 place-items-center px-(--page-pad-x)">
      <div class="flex max-w-[44ch] flex-col items-center gap-sm text-center">
        <span class="grid size-10 place-items-center rounded-panel bg-surface-hover text-muted">
          <Icon name="terminal" size={20} />
        </span>
        <h2 class="m-0 text-md font-semibold text-ink">{props.what} chưa dựng</h2>
        <p class="m-0 text-sm text-muted">{props.why}</p>
      </div>
    </div>
  );
}
