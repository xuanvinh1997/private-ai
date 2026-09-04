import { Key } from "@solid-primitives/keyed";
import { createMemo, Match, Show, Switch } from "solid-js";
import CodeFence from "./CodeFence";
import Diagram from "./Diagram";
import { splitFences, type Segment } from "./fences";
import Markdown from "./Markdown";

/** Assistant message body: markdown, code fences and mermaid. Only a settled segment is parsed as markdown and only a closed fence becomes a diagram, since half-typed input reflows and throws; `<Key>` keeps finished blocks out of the per-token rebuild. */
export default function Blocks(props: { text: string; streaming?: boolean }) {
  const rows = createMemo(() => {
    const segments = splitFences(props.text);
    return segments.map((seg, index) => ({
      key: `${index}:${seg.kind}:${seg.kind === "fence" ? seg.lang : ""}`,
      seg,
      last: index === segments.length - 1,
    }));
  });

  return (
    <div class="flex flex-col gap-sm text-base text-text">
      <Key each={rows()} by="key">
        {(row) => (
          <Switch>
            <Match when={row().seg.kind === "text"}>
              {/* Streaming text stays raw, settled text goes to markdown, and the caret rides the last segment. */}
              <Show
                when={props.streaming === true && row().last}
                fallback={<Markdown text={text(row().seg)} />}
              >
                <div class="whitespace-pre-wrap">
                  {text(row().seg)}
                  <Caret />
                </div>
              </Show>
            </Match>

            <Match when={row().seg.kind === "fence" && isDiagram(row().seg)}>
              <Diagram source={code(row().seg)} />
            </Match>

            <Match when={row().seg.kind === "fence"}>
              <CodeFence
                lang={lang(row().seg)}
                code={code(row().seg)}
                streaming={!closed(row().seg)}
              />
            </Match>
          </Switch>
        )}
      </Key>

      {/* A message opening with a code block has no text segment to carry the caret. */}
      <Show when={props.streaming === true && rows().at(-1)?.seg.kind !== "text"}>
        <Caret />
      </Show>
    </div>
  );
}

function Caret() {
  return (
    <span
      class="ml-3xs inline-block h-3.5 w-[2px] translate-y-[2px] bg-accent motion-safe:animate-pulse"
      aria-hidden="true"
    />
  );
}

/** Only a closed mermaid fence is rendered as a diagram; see the note at the top of the file. */
const isDiagram = (seg: Segment): boolean =>
  seg.kind === "fence" && seg.lang === "mermaid" && seg.closed;

const text = (seg: Segment): string => (seg.kind === "text" ? seg.text : "");
const code = (seg: Segment): string => (seg.kind === "fence" ? seg.code : "");
const lang = (seg: Segment): string => (seg.kind === "fence" ? seg.lang : "");
const closed = (seg: Segment): boolean => seg.kind !== "fence" || seg.closed;
