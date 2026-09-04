import { For, type JSX } from "solid-js";
import { t, type Msg } from "./core";

/** An emphasised sentence that stays *one* message: the marks live inside the string (`*bold*`, `` `code` ``) so a
 * translator sees the whole sentence and can move the emphasis. Two marks only, never nested. */
export function TRich(props: { msg: Msg; params?: Record<string, string | number> }) {
  const parts = () => split(t(props.msg, props.params));
  return (
    <For each={parts()}>
      {(part) =>
        part.mark === "b" ? (
          <b>{part.text}</b>
        ) : part.mark === "code" ? (
          <code class="font-mono">{part.text}</code>
        ) : (
          (part.text as JSX.Element)
        )
      }
    </For>
  );
}

interface Part {
  text: string;
  mark: "b" | "code" | null;
}

/** Split a translated string into marked parts; exported so it can be tested without a DOM. */
export function split(raw: string): Part[] {
  const parts: Part[] = [];
  // One pass, two marks; an unclosed mark stays plain text, since the sentence must still read.
  const pattern = /\*([^*]+)\*|`([^`]+)`/g;
  let at = 0;
  for (let m = pattern.exec(raw); m !== null; m = pattern.exec(raw)) {
    if (m.index > at) parts.push({ text: raw.slice(at, m.index), mark: null });
    parts.push(
      m[1] !== undefined ? { text: m[1], mark: "b" } : { text: m[2] ?? "", mark: "code" },
    );
    at = m.index + m[0].length;
  }
  if (at < raw.length) parts.push({ text: raw.slice(at), mark: null });
  return parts;
}
