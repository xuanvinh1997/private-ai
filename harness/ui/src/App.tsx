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
import {
  defaultToolScope,
  setDisplayMode,
  setSidebarOpen,
  setWorkspacePanelOpen,
  sidebarOpen,
  workspacePanelOpen,
} from "./lib/prefs";
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
import { S, t, type Msg } from "./lib/i18n";
import { setTheme } from "./lib/theme";
import { TranscriptActionsProvider } from "./lib/transcriptActions";
import ApprovalDialog from "./components/ApprovalDialog";
import { ChangesBoard } from "./components/ChangesPanel";
import Composer from "./components/Composer";
import { EmptyLead, PromptChips } from "./components/EmptyState";
import { usableForChat } from "./components/ModelPicker";
import ProjectSwitcher from "./components/ProjectSwitcher";
import PromptDialog from "./components/PromptDialog";
import Sidebar, { tabsFor, type TabId } from "./components/Sidebar";
import ConfirmDialog from "./components/projects/ConfirmDialog";
import ProjectsView from "./components/projects/ProjectsView";
import DocsView from "./components/docs/DocsView";
import SessionPalette from "./components/SessionPalette";
import Toasts from "./components/Toasts";
import SettingsView, { type SettingsPage } from "./components/SettingsView";
import Transcript from "./components/Transcript";
import Thinking from "./components/Thinking";
import WorkspaceHeader from "./components/WorkspaceHeader";
import WorkspacePanel, { type WorkspacePanelTab } from "./components/WorkspacePanel";

// Load the renderer registries; the side-effect import is deliberate, so this is the only place that lists them.
import "./components/nodes";

/** Model shown before the server answers, so the picker is never empty; a code, not a phrase, since the text
 * comes from `S.app.modelUnknown` at draw time. It never leaves the UI. */
const MODEL_CHUA_BIET = "pai:model-chua-biet";

/** `currentId` before the UI holds a real session; not a session, so sending while it stands yields a
 * "session not found" error. It only exists between signal creation and `openBlankSession`. */
const PHIEN_CHUA_MO = "phien-nhap";

/** The shell's open dialog: one slot for all three, not three flags, so two dialogs cannot be on screen at once.
 * All three edit state this file owns - the project list and the session list. */
type AppDialog =
  | { kind: "forget-project"; project: Project }
  | { kind: "rename-session"; id: string; title: string }
  | { kind: "delete-session"; id: string; title: string };

/** App shell: a left sidebar, a centred conversation column, and a multi-view inspector on the right, shaped
 * after ChatGPT and Codex. Conversation state is per session and restored on return, since losing your place
 * is the fastest way to make session switching feel expensive. */
export default function App() {
  const conversation = createConversation();
  const [sessions, setSessions] = createSignal<SessionSummary[]>([]);
  const [currentId, setCurrentId] = createSignal(PHIEN_CHUA_MO);
  const [draft, setDraft] = createSignal("");
  /** A message typed *while the previous turn was running*, waiting its turn. Exactly one slot, not a queue:
   * three questions against context the author has not read is three questions they would have written differently. */
  const [queued, setQueued] = createSignal("");
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  // The open dialog; see `AppDialog` for why there is only one slot.
  const [dialog, setDialog] = createSignal<AppDialog | null>(null);
  // Work behind a confirm button; only session deletion uses it, being the one job where closing early would leave
  // the list lying.
  const [dialogBusy, setDialogBusy] = createSignal(false);
  // Three narrowing reads of `dialog()`, written as functions because `<Show>` only narrows through its own value.
  const forgetting = () => {
    const open = dialog();
    return open?.kind === "forget-project" ? open.project : null;
  };
  const renaming = () => {
    const open = dialog();
    return open?.kind === "rename-session" ? open : null;
  };
  const deleting = () => {
    const open = dialog();
    return open?.kind === "delete-session" ? open : null;
  };
  // Esc and outside clicks both come through here, so the busy guard lives here once.
  const closeDialog = () => {
    if (!dialogBusy()) setDialog(null);
  };
  const [tab, setTab] = createSignal<TabId>("chat");
  // The settings page lives here, not in `SettingsView`: the sidebar links straight to the MCP page, and local
  // state there would swallow that click whenever settings was already open.
  const [settingsPage, setSettingsPage] = createSignal<SettingsPage>("chung");

  // Switching projects can make the open screen disappear from the sidebar, leaving no row lit and an empty frame.
  createEffect(() => {
    if (!tabsFor(project()?.kind).includes(tab())) setTab("chat");
  });
  const [loading, setLoading] = createSignal(true);
  const [models, setModels] = createSignal<ModelChoice[]>([]);
  const [model, setModel] = createSignal(MODEL_CHUA_BIET);
  // The session being reloaded from the log; an id rather than a boolean, so a fast double switch cannot overwrite.
  const [loadingSession, setLoadingSession] = createSignal<string | null>(null);
  const [loadError, setLoadError] = createSignal<string | null>(null);
  // Tool scope for the *next* turn: it ships with each send and is never stored. The starting point comes from the
  // Permissions page (`defaultToolScope`), the only place that writes it - so a one-off shell grant in the composer
  // dies with the window instead of quietly persisting.
  const [scope, setScope] = createSignal<ToolScope>(defaultToolScope());

  const [projects, setProjects] = createSignal<Project[]>([]);
  // Switching projects makes the core swap a whole plugin layer; this flag locks interaction while it does.
  const [switching, setSwitching] = createSignal(false);
  // Which project row has its context menu open; an id, since one shared flag would open all of them.
  const [projectMenu, setProjectMenu] = createSignal<string | null>(null);

  // MCP servers are this app's plugins, so the badge must count *connected* ones: a `failed` server adds no tools.
  const [mcpServers, setMcpServers] = createSignal<McpServer[]>([]);
  const mcpConnected = () => mcpServers().filter((server) => server.state === "connected").length;
  // Re-asked on every return to the conversation, not once at startup: the settings page next door can toggle
  // servers, and a frozen number would misstate where the assistant's tools come from.
  createEffect(() => {
    if (tab() === "chat") void refreshMcp();
  });
  async function refreshMcp() {
    setMcpServers(await listMcpServers());
  }

  const project = () => projects().find((entry) => entry.isCurrent) ?? null;
  const projectKey = () => project()?.id ?? "khong-co-du-an";
  /** Whether a project is open - a valid state, not a pending load. Without one, no project-layer plugin is
   * attached, so every screen promising read/edit/shell must read this flag rather than infer from `kind`. */
  const hasProject = () => project() !== null;

  /** Clicking a file in the tree drops `@relative/path` into the composer; relative because that is what the `@`
   * completion and the tools speak, and appended rather than overwriting, since the question is usually typed first. */
  function mentionFile(root: string, path: string) {
    const bare = path.startsWith(root) ? path.slice(root.length) : path;
    const rel = bare.replace(/^[\\/]+/, "").replace(/\\/g, "/");
    setDraft((current) => {
      const head = current === "" || current.endsWith(" ") ? current : `${current} `;
      return `${head}@${rel} `;
    });
  }

  // The inspector has one shell; switching between diff and files changes a tab, not the column.
  const [workspacePanelTab, setWorkspacePanelTab] =
    createSignal<WorkspacePanelTab>("changes");
  // Below this width, opening the inspector borrows the left sidebar's space; presentation only, never written back.
  const [narrowWorkspace, setNarrowWorkspace] = createSignal(false);
  let workspaceMain: HTMLElement | undefined;

  // Transcripts of closed sessions, in memory only: the Rust session log is the source of truth.
  const parked = new Map<string, ConversationNode[]>();

  const files = createMemo(() => changedFiles(conversation.nodes()));
  const workspacePanelVisible = () => workspacePanelOpen() && hasProject();
  const sidebarVisible = () =>
    sidebarOpen() && !(narrowWorkspace() && workspacePanelVisible());
  const closeWorkspacePanel = () => {
    setWorkspacePanelOpen(false);
    queueMicrotask(() => workspaceMain?.focus());
  };

  /** A session row's secondary line: the last thing said. The *open* transcript wins over the core's copy, since
   * a just-finished turn is not in the list yet and a stale preview looks like a hung UI. */
  function preview(session: SessionSummary): string | undefined {
    const nodes = session.id === currentId() ? conversation.nodes() : parked.get(session.id);
    if (!nodes) return session.preview ?? undefined;
    for (let at = nodes.length - 1; at >= 0; at -= 1) {
      const node = nodes[at]!;
      if (node.kind === "user") return t(S.app.previewYou, { text: node.text });
      if (node.kind === "assistant" && node.text !== "") return node.text;
    }
    return session.preview ?? undefined;
  }

  onMount(async () => {
    if (isDemo()) {
      const knobs = demoKnobs();
      if (knobs.theme) setTheme(knobs.theme);
      if (knobs.mode) setDisplayMode(knobs.mode);
      if (knobs.changes !== undefined) setWorkspacePanelOpen(knobs.changes);
      if (knobs.panel) setWorkspacePanelTab(knobs.panel);
      if (knobs.sidebar !== undefined) setSidebarOpen(knobs.sidebar);
      // `?demo=1&project=0` builds the no-project state; it is a knob because that is the *first* state users meet.
      setProjects(demoProjects(knobs.project ?? "p-harness"));
      // Screenshot knobs: all three states below last only a click, so without them the only capture is a code edit.
      if (knobs.tab !== undefined && isTab(knobs.tab)) setTab(knobs.tab);
      if (knobs.menu === "project") setProjectMenu(knobs.project ?? "p-harness");
      if (knobs.switching) setSwitching(true);
      if (knobs.state === "skeleton") return; // the skeleton stays put so it can be inspected
      const seed = demoSessions(projectKey());
      for (const [id, nodes] of Object.entries(demoParked())) parked.set(id, nodes);
      setSessions(seed);
      setModels(demoModels());
      setModel(demoModels().filter(usableForChat)[0]?.id ?? MODEL_CHUA_BIET);
      setCurrentId(seed[0]?.id ?? PHIEN_CHUA_MO);
      conversation.reset(knobs.state === "empty" ? [] : demoNodes());
      setLoading(false);
      return;
    }
    setProjects(await listProjects());
    const [list, available] = await Promise.all([listSessions(), listModels()]);
    if (list.length > 0) {
      setSessions(list);
      // Do *not* set `currentId` first: `switchTo` short-circuits on that comparison and would never load the
      // transcript. It sets `currentId` synchronously on its first line anyway.
      await switchTo(list[0]!.id);
    } else {
      // First run, no sessions in the log: build one now, so the first question reaches the model rather than an error.
      await openBlankSession();
    }
    setModels(available);
    // Only pick among chat-capable models: defaulting to an embedding-only one opens the app with a dead conversation.
    const chat = available.filter(usableForChat);
    // Within those, prefer a tool-capable model, or the user meets an assistant that never reads a file.
    setModel(chat.find((choice) => choice.tools)?.id ?? chat[0]?.id ?? MODEL_CHUA_BIET);
    setLoading(false);
  });

  /** Re-ask the core which models the active provider has. Asking only at startup broke the "plug in a new server"
   * flow: the picker still showed the pre-settings list, usually empty. The current model is kept when it survives
   * the new list, so changing a *different* provider does not move the next turn's model. */
  const refreshModels = async () => {
    if (isDemo() || !inTauri()) return;
    const available = await listModels();
    setModels(available);
    const chat = available.filter(usableForChat);
    if (chat.some((choice) => choice.id === model())) return;
    setModel(chat.find((choice) => choice.tools)?.id ?? chat[0]?.id ?? MODEL_CHUA_BIET);
  };

  /** Leaving settings is the moment to re-ask; one hook at the door covers provider, model, toggle and delete alike.
   * The flag is a plain variable, not a signal, since a signal here would make the effect re-run itself. */
  let leftSettings = false;
  createEffect(() => {
    const at = tab();
    if (leftSettings && at !== "settings") void refreshModels();
    leftSettings = at === "settings";
  });

  /** Warning under the model picker, or `undefined` when there is nothing to say; the two silent failures differ,
   * so they get two messages. */
  const modelWarning = () => {
    // Not special-cased for the demo: the demo exists precisely to see these states without standing up a server.
    if (models().length === 0) return inTauri() ? t(S.app.modelNoServer) : undefined;
    // A healthy server with nothing chat-capable is a third case; silence here would leave a pill that lies.
    if (!models().some(usableForChat)) return t(S.app.modelEmbedOnly);
    const picked = models().find((choice) => choice.id === model());
    if (picked && !picked.tools) return t(S.app.modelNoTools);
    return undefined;
  };

  onMount(() => {
    const query = window.matchMedia("(max-width: 959px)");
    const sync = () => setNarrowWorkspace(query.matches);
    sync();
    query.addEventListener("change", sync);
    onCleanup(() => query.removeEventListener("change", sync));
  });

  // Ctrl/Cmd+K opens session search, bound on `window` so it works with focus inside the composer.
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
    // Clicking a session row means "show me this session", so it must return the whole screen to the conversation -
    // before the guard below, and even for the session already open. Without it, clicking a session from another
    // screen does nothing visible, and the only route back would be creating a new session.
    setTab("chat");
    if (id === currentId()) return;
    parked.set(currentId(), conversation.nodes().slice());
    setCurrentId(id);
    setLoadError(null);
    // The token counts belong to the session just left; keeping them would show 90% context on an empty session.
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
      // The user may have switched while we waited; writing now would show one session's transcript under another's name.
      if (currentId() !== id) return;
      parked.set(id, nodes);
      conversation.reset(nodes);
    } catch (err) {
      if (currentId() === id) setLoadError(String(err));
    } finally {
      if (loadingSession() === id) setLoadingSession(null);
    }
  }

  /** Clear the screen after the core has finished switching projects; run *after* the answer, since the transcript
   * and session cache belong to the old project and a failed switch must not discard them. */
  async function adoptProject() {
    parked.clear();
    setLoadError(null);
    conversation.reset([]);

    if (isDemo()) {
      const seed = demoSessions(projectKey());
      setSessions(seed);
      setCurrentId(seed[0]?.id ?? PHIEN_CHUA_MO);
      return;
    }

    // Sessions *of the open project*: the core swapped layers, so `list_sessions` now returns a different set.
    const list = await listSessions();
    setSessions(list);
    const first = list[0];
    // A project with no sessions gets a blank one, as at startup: people type immediately after switching.
    if (!first) {
      await openBlankSession();
      return;
    }
    setCurrentId(first.id);
    const nodes = nodesFromHistory(await loadSession(first.id));
    parked.set(first.id, nodes);
    conversation.reset(nodes);
  }

  /** Switch to a project already in the list. */
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
      setLoadError(t(S.app.error.switchProject, { name: target.name, err: String(err) }));
    } finally {
      setSwitching(false);
    }
  }

  /** Close the open project and stay in the app, chat only. It takes `switchProject`'s exact path, since to the
   * screen this is one more project switch whose destination happens to be "no project". No confirmation: the
   * list loses no row and reopening is two clicks. */
  /// Change the open project's kind, along `switchProject`'s exact path, because the core swaps the whole plugin
  /// layer either way and a different path would leave the busy flag off.
  async function swapProjectKind(kind: ProjectKind) {
    const open = project();
    if (open === null || switching()) return;
    setSwitching(true);
    setLoadError(null);
    try {
      setProjects(await setProjectKind(open.id, kind));
      await adoptProject();
    } catch (err) {
      setLoadError(t(S.app.error.swapKind, { err: String(err) }));
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
        // The core returns the whole list after closing: no row is dropped, only `isCurrent` is cleared.
        setProjects(await closeProject());
      }
      await adoptProject();
    } catch (err) {
      setLoadError(t(S.app.error.closeProject, { err: String(err) }));
    } finally {
      setSwitching(false);
    }
  }

  /** Open a dropped directory as a project - the last remaining entrance to `open_project`, since every deliberate
   * route goes through the projects screen. Never guess whether a path is a directory; only the core sees the disk. */
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
      setLoadError(t(S.app.error.openFolder, { path, err: String(err) }));
    } finally {
      setSwitching(false);
    }
  }

  /** Remove a project *from the list*; nothing on disk is touched. The screen updates before the core is told,
   * unlike session deletion, because nothing is lost if it fails. The confirmation lives at the call sites. */
  function forgetProject(target: Project) {
    setProjects((all) => all.filter((entry) => entry.id !== target.id));
    if (isDemo()) return;
    void removeProject(target.id).catch(async (err: unknown) => {
      setLoadError(t(S.app.error.forgetProject, { name: target.name, err: String(err) }));
      setProjects(await listProjects());
    });
  }

  /** Build a blank session in the open workspace and adopt it. Every route to "no sessions left" comes through here,
   * since a real session is cheaper than teaching the composer about a "cannot send yet" state. The core attaches
   * `cwd` from the open project; with none, it is a plain chat session, which is still valid. */
  async function openBlankSession(): Promise<void> {
    // A temporary name, unnumbered: numbering by current list length produces two "Session 3" after a deletion.
    const title = t(S.app.sessionNew);
    const created = (await createSession(title)) ?? {
      id: `local-${Date.now()}`,
      title,
      updatedAt: Date.now(),
      preview: null,
    };
    setSessions((all) => [created, ...all]);
    setCurrentId(created.id);
    conversation.reset([]);
  }

  async function newSession() {
    parked.set(currentId(), conversation.nodes().slice());
    await openBlankSession();
    setTab("chat");
  }

  /** Open the rename dialog prefilled: editing one character is more common than retyping the whole name. */
  function askRename(id: string) {
    const current = sessions().find((session) => session.id === id);
    setDialog({ kind: "rename-session", id, title: current?.title ?? "" });
  }

  /** Rename: update the screen first, then tell the core. A failed write costs only the name. */
  function rename(id: string, next: string) {
    setSessions((all) => all.map((s) => (s.id === id ? { ...s, title: next } : s)));
    void renameSession(id, next);
  }

  /** Open the delete confirmation; the session name is captured at ask time, since "this session" is not enough
   * to know what is about to be lost. */
  function askRemove(id: string) {
    const current = sessions().find((session) => session.id === id);
    setDialog({ kind: "delete-session", id, title: current?.title ?? id });
  }

  /** Delete: ask the core *first*, then drop it from the screen. The opposite of rename, deliberately, because
   * deletion cannot be undone and a refusal must not leave the user believing otherwise. */
  async function remove(id: string) {
    // The dialog stays and disables its own button while waiting: two clicks would send two deletes, and the second
    // would report an error about the session the first just removed.
    setDialogBusy(true);
    try {
      await deleteSession(id);
    } catch (err) {
      setLoadError(t(S.app.error.deleteSession, { err: String(err) }));
      return;
    } finally {
      setDialogBusy(false);
      setDialog(null);
    }
    parked.delete(id);
    const rest = sessions().filter((session) => session.id !== id);
    setSessions(rest);
    if (currentId() === id) {
      const next = rest[0];
      // Deleting the last session must leave a screen you can still type into, so it leaves a real session behind.
      if (!next) {
        await openBlankSession();
        return;
      }
      setCurrentId(next.id);
      conversation.reset(parked.get(next.id) ?? []);
    }
  }

  /** Wait for an approval answer; demo turns only, since a real turn blocks in Rust, where the only trustworthy
   * timeout lives. */
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

  /** Name a session after its first question, as ChatGPT does. The condition is "the transcript holds no user
   * message", not a name pattern, which would rename a session someone deliberately called "Session tests".
   * Called *before* `addUser`, since afterwards the transcript already holds that message. */
  function nameFromFirstMessage(text: string) {
    if (conversation.nodes().some((node) => node.kind === "user")) return;
    const title = titleFromMessage(text);
    if (title === "") return;
    const id = currentId();
    if (!sessions().some((session) => session.id === id)) return;
    // Screen first, then the core, as in `rename`: a failed write costs only the name.
    setSessions((all) => all.map((s) => (s.id === id ? { ...s, title } : s)));
    void renameSession(id, title);
  }

  async function send(text: string) {
    const trimmed = text.trim();
    if (trimmed === "") return;
    // The previous turn is still running: keep this message, do not swallow it.
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

    // Capture the session *before* sending and reuse it in `finally`: `currentId()` may have changed by the end.
    const cuaLuot = currentId();

    try {
      // A turn's events are only written to the session that sent it; without this guard, tokens and tool cards from
      // an old turn would land in a newly opened transcript and be saved there. The old turn still finishes in the
      // core and reaches the log, so only the live rendering is dropped.
      const applyIfCurrent = (event: AgentEvent) => {
        if (currentId() !== cuaLuot) return;
        conversation.applyEvent(event);
      };
      // Capture the scope alongside the session: a mid-turn change belongs to the next turn, not this one.
      const quyen = scope();
      if (isDemo() || !inTauri()) {
        await runDemoTurn(trimmed, quyen, applyIfCurrent, waitForApproval);
      } else {
        await sendMessage(cuaLuot, trimmed, quyen, applyIfCurrent);
      }
    } catch (err) {
      conversation.applyEvent({ kind: "error", message: String(err) });
    } finally {
      // However the turn ends, a pending approval must close - as a rejection, since nobody is left to answer.
      if (conversation.approval()) decideApproval("rejected");
      conversation.finishTurn();
      // The parked snapshot must also stop streaming, or returning to it shows a cursor blinking forever.
      const chup = parked.get(cuaLuot);
      if (chup) {
        parked.set(
          cuaLuot,
          chup.map((node) =>
            node.kind === "assistant" && node.streaming ? { ...node, streaming: false } : node,
          ),
        );
      }

      // A queued message belongs to the session that received it; on a mid-turn switch it falls back into the
      // composer, text intact, and only when the draft is empty, so nothing typed is overwritten.
      const cho = queued();
      if (cho !== "") {
        setQueued("");
        if (currentId() === cuaLuot) void send(cho);
        else setDraft((hien) => (hien.trim() === "" ? cho : hien));
      }
    }
  }

  /** Run a `/` command from the composer. Every branch must point at an action that already exists: a second route
   * to the same job is a second place for the two to diverge. */
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
        setWorkspacePanelTab("changes");
        setWorkspacePanelOpen(true);
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

  /** Scroll to the node a file in the changes panel points at. */
  function reveal(nodeId: string) {
    setTab("chat");
    queueMicrotask(() => {
      const el = document.getElementById(`node-${nodeId}`);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      // Flash the outline once: after a smooth scroll, the user needs to know *which* item was brought into view.
      // `outline-style` cannot animate, so a transparent outline must be in place first, or the effect runs on
      // `outline-style: none` and silently does nothing.
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

  /** Nothing in the transcript yet - the state that decides whether the composer sits centred or at the bottom. */
  const chatEmpty = () => conversation.nodes().length === 0;
  /** Whether to show the prompt chips: only with an empty transcript and nothing else occupying that space, or
   * they invite starting work that a loading transcript is about to cover. */
  const showPrompts = () => chatEmpty() && loadingSession() === null && loadError() === null;

  const title = () =>
    tab() === "chat"
      ? (sessions().find((session) => session.id === currentId())?.title ?? t(S.app.sessionTitle))
      : t(TAB_TITLE[tab()]);

  return (
    <TranscriptActionsProvider
      value={{
        resend: conversation.busy() ? null : (text) => void send(text),
        remove: conversation.removeNode,
        // No screen reads files any more, so paths render as text, not buttons: a button that does nothing is worse.
        openFile: null,
      }}
    >
      <div class="flex h-full min-h-0 overflow-hidden bg-bg">
        {/* The workspace is wrapped in its own layer for one reason: when settings opens, the layer becomes `inert`.
            Settings covers the window visually, but Tab would still walk into a composer nobody can see. `inert`
            removes the branch from both the tab order and the accessibility tree, which `aria-hidden` cannot. */}
        <div
          class="flex min-h-0 min-w-0 flex-1 overflow-hidden"
          ref={(el) => {
            // Set via `toggleAttribute`, not a JSX prop: `inert` is missing from Solid's JSX types, and an `as any`
            // here would disable type checking for every other prop on this element.
            createEffect(() => el.toggleAttribute("inert", tab() === "settings"));
          }}
        >
        <Show when={sidebarVisible()}>
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
                onPick={(id) => {
                  // Clicking a project row always opens its detail panel; an unopened row switches project first.
                  // The branch lives here rather than in `switchProject`, whose name promises exactly one thing.
                  setWorkspacePanelTab("files");
                  setWorkspacePanelOpen(true);
                  if (projects().find((entry) => entry.id === id)?.isCurrent !== true) {
                    void switchProject(id);
                  }
                }}
                onSeeAll={() => setTab("projects")}
                // The sidebar menu has no screen to ask on its behalf, so it opens the shell's confirmation;
                // the projects screen calls `forgetProject` directly, having already asked at the row.
                onForget={(target) => setDialog({ kind: "forget-project", project: target })}
                onClose={() => void closeCurrentProject()}
                onSwapKind={(kind) => void swapProjectKind(kind)}
              />
            )}
            onSelect={(id) => void switchTo(id)}
            onCreate={() => void newSession()}
            onRename={askRename}
            onDelete={askRemove}
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

        <main
          ref={workspaceMain}
          tabIndex={-1}
          class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
        >
          <WorkspaceHeader
            title={title()}
            scope={project()?.name}
            busy={conversation.busy() || switching()}
            busyLabel={switching() ? t(S.app.switchingProject) : undefined}
            sidebarOpen={sidebarVisible()}
            onOpenSidebar={() => {
              setSidebarOpen(true);
              if (narrowWorkspace()) setWorkspacePanelOpen(false);
            }}
            changesPanelOpen={workspacePanelVisible() && workspacePanelTab() === "changes"}
            changeCount={files().length}
            // Without a project no tool touches the disk, so the changes panel is permanently empty.
            onToggleChangesPanel={
              tab() === "chat" && hasProject()
                ? () => {
                    if (workspacePanelVisible() && workspacePanelTab() === "changes") {
                      setWorkspacePanelOpen(false);
                      return;
                    }
                    setWorkspacePanelTab("changes");
                    setWorkspacePanelOpen(true);
                  }
                : undefined
            }
          />

          <div class="relative flex min-h-0 flex-1">
            <div class="flex min-w-0 flex-1 flex-col">
              <Switch>
                <Match when={tab() === "chat"}>
                  {/* With an empty transcript the question and composer float to the vertical centre, as in ChatGPT;
                      once there is a conversation the composer returns to the bottom. It is one instance across both
                      layouts - only the wrapper's classes change - because rebuilding it on the first send would
                      steal keyboard focus exactly as the user goes to type again. */}
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
                                {t(S.app.loadingTranscript)}
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
                      <Transcript
                        nodes={conversation.nodes()}
                        footer={
                          <Thinking nodes={conversation.nodes()} busy={conversation.busy()} />
                        }
                      />
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
                      model={model() === MODEL_CHUA_BIET ? t(S.app.modelUnknown) : model()}
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

                    {/* The chips sit *below* the composer: the big question must touch the place that answers it. */}
                    <Show when={showPrompts()}>
                      <div class="shrink-0 px-(--page-pad-x) pb-(--page-pad-y)">
                        <PromptChips
                          disabled={conversation.busy()}
                          kind={project()?.kind ?? null}
                          projectKey={projectKey()}
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
                    onOpenPath={(path) => void openFolder(path)}
                    onForget={forgetProject}
                    onCreated={async () => {
                      setProjects(await listProjects());
                      await adoptProject();
                    }}
                  />
                </Match>

                <Match when={tab() === "library"}>
                  {/* `resetKey` is the project path: using the id would reload an unchanged library whenever a
                      project was removed and re-added. */}
                  <DocsView resetKey={project()?.path ?? ""} name={project()?.name} />
                </Match>

              </Switch>
            </div>

            <Show when={workspacePanelVisible() ? project() : null} keyed>
              {(open) => (
                <WorkspacePanel
                  tab={workspacePanelTab()}
                  files={files()}
                  project={open}
                  onTab={setWorkspacePanelTab}
                  onReveal={(nodeId) => {
                    reveal(nodeId);
                    if (narrowWorkspace()) closeWorkspacePanel();
                  }}
                  onClose={closeWorkspacePanel}
                  onOpenFolder={() => void openFolder(open.path)}
                  onPickFile={(path) => mentionFile(open.path, path)}
                  onOpenScreen={() => {
                    setTab(open.kind === "docs" ? "library" : "diff");
                    closeWorkspacePanel();
                  }}
                  focusOnMount={narrowWorkspace()}
                />
              )}
            </Show>
          </div>
        </main>
        </div>

        {/* Settings is a full-window mode, so it sits outside `<main>` rather than inside the workspace `<Switch>`:
            the sidebar and composer have no business there, and leaving them visible invites a misclick while an
            API key is being edited. The tree below stays mounted, so returning keeps your place. `z-30`, below
            dialogs at `z-50`, since approval and session search must still float above settings. */}
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

        {/* The shell's three dialogs, last in the tree and side by side rather than inside the screens: they edit
            the project and session lists this file owns, and a dialog built inside the row it asks about would be
            unmounted just as it needs to restore focus. `dialog()` holds one value, so the three are exclusive. */}
        <Show when={forgetting()}>
          {(target) => (
            <ConfirmDialog
              icon="trash"
              title={t(S.app.forget.title, { name: target().name })}
              body={t(S.app.forget.body)}
              more={t(S.app.forget.more)}
              detail={target().path}
              confirmLabel={t(S.app.forget.confirm)}
              onClose={closeDialog}
              onConfirm={() => {
                // Read the value *before* closing: closing unmounts this branch and `target()` has nothing left.
                const picked = target();
                setDialog(null);
                forgetProject(picked);
              }}
            />
          )}
        </Show>

        <Show when={renaming()}>
          {(open) => (
            <PromptDialog
              icon="pencil"
              title={t(S.app.rename.title)}
              label={t(S.app.rename.label)}
              value={open().title}
              placeholder={t(S.app.sessionNew)}
              confirmLabel={t(S.common.rename)}
              onClose={closeDialog}
              onConfirm={(next) => {
                const id = open().id;
                setDialog(null);
                rename(id, next);
              }}
            />
          )}
        </Show>

        <Show when={deleting()}>
          {(open) => (
            <ConfirmDialog
              icon="trash"
              title={t(S.app.remove.title, { title: open().title })}
              body={t(S.app.remove.body)}
              more={t(S.app.remove.more)}
              confirmLabel={t(S.app.remove.confirm)}
              busy={dialogBusy()}
              onClose={closeDialog}
              onConfirm={() => void remove(open().id)}
            />
          )}
        </Show>

        {/* Last in the tree and outside every screen `<Show>`: a toast must survive the tab change or dialog close
            that produced it. It sources its own content from `lib/toast.ts`, so no props are passed here. */}
        <Toasts />

        <Show when={paletteOpen()}>
          <SessionPalette
            sessions={sessions()}
            currentId={currentId()}
            onPick={(id) => {
              // `switchTo` returns to the conversation itself. This palette used to be the only caller that did so.
              void switchTo(id);
              setPaletteOpen(false);
            }}
            onClose={() => setPaletteOpen(false)}
          />
        </Show>
      </div>
    </TranscriptActionsProvider>
  );
}

const TAB_TITLE: Record<TabId, Msg> = {
  chat: S.app.tab.chat,
  diff: S.app.tab.diff,
  library: S.app.tab.library,
  projects: S.common.project,
  settings: S.common.settings,
};

/** The demo's `?tab=` knob comes from the URL, so it is an arbitrary string until validated. */
const isTab = (raw: string): raw is TabId => Object.hasOwn(TAB_TITLE, raw);
