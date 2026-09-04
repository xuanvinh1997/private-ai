import { Key } from "@solid-primitives/keyed";
import { createSignal, onCleanup, onMount, Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { S, t } from "../lib/i18n";
import { displayMode } from "../lib/prefs";
import type { ConversationNode } from "../lib/protocol";
import { nodeRenderer } from "../lib/registry";
import Icon from "./Icon";

/** Stick-to-bottom threshold, the same number as chat_view.py:1226. */
const STICK_PX = 80;

/** The transcript, which deliberately knows no content kinds: it looks each one up in the registry by `kind`.
 * Stick-to-bottom uses a `ResizeObserver`, since an effect on the array would force layout on every token. */
export default function Transcript(props: {
  nodes: ConversationNode[];
  empty?: JSX.Element;
  /** Appended inside the observed region, so the working indicator's height counts toward stick-to-bottom. */
  footer?: JSX.Element;
}) {
  let scroller: HTMLDivElement | undefined;
  let content: HTMLDivElement | undefined;

  // Scrolling up releases the stick: forcing the view down loses the reader's place with no way back.
  let stuck = true;
  const [atBottom, setAtBottom] = createSignal(true);

  const toBottom = (smooth: boolean) => {
    stuck = true;
    setAtBottom(true);
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  };

  onMount(() => {
    const el = scroller;
    const body = content;
    if (!el || !body) return;

    const onScroll = () => {
      stuck = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
      setAtBottom(stuck);
    };
    el.addEventListener("scroll", onScroll, { passive: true });

    const observer = new ResizeObserver(() => {
      if (stuck) el.scrollTop = el.scrollHeight;
    });
    observer.observe(body);

    onCleanup(() => {
      el.removeEventListener("scroll", onScroll);
      observer.disconnect();
    });
  });

  return (
    <div class="relative min-h-0 flex-1">
      <div
        ref={scroller}
        class="h-full overflow-y-auto px-(--page-pad-x)"
        // The browser's own scroll anchoring fights the stick-to-bottom logic mid-stream and the view judders.
        style={{ "overflow-anchor": "none" }}
      >
        <div
          ref={content}
          class="mx-auto flex flex-col gap-lg py-lg"
          // Document mode drops the reading measure: it exists so diffs and command output get room.
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <Show when={props.nodes.length > 0} fallback={props.empty}>
            {/* Keyed by `id`, so prepending older nodes does not remount the whole transcript. */}
            <Key each={props.nodes} by="id">
              {(node) => <NodeSeat node={node()} />}
            </Key>
          </Show>

          {props.footer}
        </div>
      </div>

      <BackBottom visible={!atBottom()} onClick={() => toBottom(true)} />
    </div>
  );
}

/** Scroll-to-bottom button, always mounted and only fading, so keyboard focus is never dropped from under the user. */
function BackBottom(props: { visible: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      tabIndex={props.visible ? 0 : -1}
      aria-hidden={!props.visible}
      class="absolute right-lg bottom-lg z-[var(--z-floating)] flex items-center gap-2xs rounded-pill border border-line bg-[var(--glass)] px-md py-2xs text-2xs text-text shadow-float backdrop-blur transition-all duration-[var(--dur-base)] ease-[var(--ease-out)] hover:bg-surface"
      classList={{
        "pointer-events-none translate-y-2 opacity-0": !props.visible,
        "translate-y-0 opacity-100": props.visible,
      }}
    >
      <Icon name="arrow-down" size={13} />
      {t(S.chat.transcript.toBottom)}
    </button>
  );
}

function NodeSeat(props: { node: ConversationNode }) {
  const render = () => nodeRenderer(props.node.kind);
  return (
    // The id lives on the slot, not the renderer: the changes panel scrolls to any node without knowing who draws it.
    <div id={`node-${props.node.id}`} class="scroll-mt-lg">
      <Show when={render()} fallback={<UnknownNode kind={props.node.kind} />}>
        {(component) => <Dynamic component={component()} node={props.node} />}
      </Show>
    </div>
  );
}

/** The key space is open, so a missing renderer is valid; a grey line beats silently swallowing an event. */
function UnknownNode(props: { kind: string }) {
  return (
    <p class="m-0 px-sm text-2xs text-faint">
      {t(S.chat.transcript.unknown, { kind: props.kind })}
    </p>
  );
}
