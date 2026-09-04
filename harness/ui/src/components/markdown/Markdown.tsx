import { Marked, type Token, type Tokens } from "marked";
import { createMemo, For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import CodeFence from "./CodeFence";
import MathSpan from "./MathSpan";
import { MATH_EXTENSIONS } from "./math";

/** Markdown rendered into Solid components, never through HTML: `lexer()` stops at a token tree, which is data, so model text can never become markup and there is nothing to sanitise. `source()` memoises the input so `lexer` only reruns when the text really changes. */
/** A private `Marked` instance, not `marked.use()`, so the math extensions apply only here. */
const md = new Marked({ extensions: MATH_EXTENSIONS });

export default function Markdown(props: { text: string }) {
  const source = createMemo(() => props.text);
  const tokens = createMemo<Token[]>(() => md.lexer(source()));

  return <BlockSeq tokens={tokens()} gap="space-y-sm" />;
}

/** A vertical block sequence; spacing uses `space-y-*`, since flex children blockify and break inline runs. */
function BlockSeq(props: { tokens: Token[]; gap: string }) {
  return (
    <div class={props.gap}>
      <For each={props.tokens}>{(token) => <BlockToken token={token} />}</For>
    </div>
  );
}

/** A model's `#` starts at `h2`; see the note in the `heading` branch. */
const HEADING_TAG = ["h2", "h3", "h4", "h5", "h6", "h6"] as const;

/** One block-position token; reading `props.token` once is safe because the token tree is an immutable snapshot. */
function BlockToken(props: { token: Token }) {
  const token = props.token as Tokens.Generic;

  switch (token.type) {
    // Blank lines and reference link definitions draw nothing.
    case "space":
    case "def":
      return null;

    case "heading": {
      const heading = props.token as Tokens.Heading;
      // The page already owns `h1`, so a model's `#` starts at `h2` and the outline stays sane.
      const tag = HEADING_TAG[Math.min(Math.max(heading.depth, 1), 6) - 1] ?? "h6";
      const size = heading.depth === 1 ? "text-lg" : heading.depth === 2 ? "text-md" : "text-base";
      return (
        // Extra top margin unless first: a heading spaced like a paragraph stops dividing the text.
        <Dynamic
          component={tag}
          class={`m-0 font-semibold text-ink not-first:mt-sm ${size}`}
        >
          <InlineSeq tokens={heading.tokens} />
        </Dynamic>
      );
    }

    case "hr":
      return <hr class="m-0 border-0 border-t border-line" />;

    case "paragraph": {
      const paragraph = props.token as Tokens.Paragraph;
      return (
        <p class="m-0 leading-[1.6]">
          <InlineSeq tokens={paragraph.tokens} />
        </p>
      );
    }

    /* Indented code blocks only: fenced ones never reach here, `splitFences` took them first. */
    case "code": {
      const code = props.token as Tokens.Code;
      return <CodeFence lang={(code.lang ?? "").trim().split(/\s+/)[0] ?? ""} code={code.text} />;
    }

    case "blockquote": {
      const quote = props.token as Tokens.Blockquote;
      return (
        <blockquote class="m-0 border-l-2 border-line-strong pl-md text-muted">
          <BlockSeq tokens={quote.tokens} gap="space-y-2xs" />
        </blockquote>
      );
    }

    case "list": {
      const list = props.token as Tokens.List;
      const items = (
        <For each={list.items}>
          {(item) => (
            <li
              class="leading-[1.6]"
              // The checkbox replaces the bullet: two markers on one item read as two items.
              classList={{ "list-none -ml-lg": item.task }}
            >
              <For each={item.tokens}>{(child) => <BlockToken token={child} />}</For>
            </li>
          )}
        </For>
      );
      // No `flex` here: blockified children lose `display: list-item` and every bullet with it.
      return list.ordered ? (
        <ol
          start={typeof list.start === "number" && list.start !== 1 ? list.start : undefined}
          class="m-0 list-decimal space-y-3xs py-0 pr-0 pl-lg"
        >
          {items}
        </ol>
      ) : (
        <ul class="m-0 list-disc space-y-3xs py-0 pr-0 pl-lg">{items}</ul>
      );
    }

    case "table": {
      const table = props.token as Tokens.Table;
      return (
        // Scrolls in its own frame like `CodeFence`, so a wide table cannot stretch the transcript.
        <div class="overflow-x-auto rounded-panel border border-line">
          <table class="w-max min-w-full border-collapse text-xs">
            <thead>
              <tr>
                <For each={table.header}>
                  {(cell) => (
                    <th
                      class="border-b border-line px-sm py-2xs font-medium text-ink"
                      style={{ "text-align": cell.align ?? "left" }}
                    >
                      <InlineSeq tokens={cell.tokens} />
                    </th>
                  )}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={table.rows}>
                {(row) => (
                  <tr class="border-t border-line first:border-t-0">
                    <For each={row}>
                      {(cell) => (
                        <td
                          class="px-sm py-2xs align-top"
                          style={{ "text-align": cell.align ?? "left" }}
                        >
                          <InlineSeq tokens={cell.tokens} />
                        </td>
                      )}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      );
    }

    /* Tight list item: text sits straight in the `<li>` with no `<p>`, keeping the checkbox inline. */
    case "text": {
      const text = props.token as Tokens.Text;
      return (
        <Show when={text.tokens} fallback={text.text}>
          {(children) => <InlineSeq tokens={children()} />}
        </Show>
      );
    }

    case "mathBlock":
      return <MathSpan tex={token.text} display />;

    case "checkbox": {
      const box = props.token as Tokens.Checkbox;
      return (
        <input
          type="checkbox"
          checked={box.checked}
          disabled
          // Disabled on purpose, and `aria-hidden` because the text beside it already says the state.
          aria-hidden="true"
          class="mr-2xs align-[-1px] accent-[var(--accent)]"
        />
      );
    }

    default:
      return <RawText token={props.token} />;
  }
}

function InlineSeq(props: { tokens: Token[] }) {
  return <For each={props.tokens}>{(token) => <InlineToken token={token} />}</For>;
}

function InlineToken(props: { token: Token }) {
  const token = props.token as Tokens.Generic;

  switch (token.type) {
    case "text":
    case "escape": {
      const text = props.token as Tokens.Text;
      return (
        <Show when={text.tokens} fallback={text.text}>
          {(children) => <InlineSeq tokens={children()} />}
        </Show>
      );
    }

    case "strong": {
      const strong = props.token as Tokens.Strong;
      return (
        <strong class="font-semibold text-ink">
          <InlineSeq tokens={strong.tokens} />
        </strong>
      );
    }

    case "em": {
      const em = props.token as Tokens.Em;
      return (
        <em>
          <InlineSeq tokens={em.tokens} />
        </em>
      );
    }

    case "del": {
      const del = props.token as Tokens.Del;
      return (
        <del class="text-muted">
          <InlineSeq tokens={del.tokens} />
        </del>
      );
    }

    case "codespan": {
      const code = props.token as Tokens.Codespan;
      return (
        <code class="rounded-btn bg-[var(--overlay-faint)] px-3xs py-px font-mono text-2xs">
          {code.text}
        </code>
      );
    }

    case "mathInline":
      return <MathSpan tex={token.text} display={false} />;

    case "br":
      return <br />;

    case "link": {
      const link = props.token as Tokens.Link;
      return <LinkOut href={link.href} title={link.title ?? undefined} tokens={link.tokens} />;
    }

    /* Images render as links, not `<img>`: loading a model-supplied URL is an unasked-for beacon. */
    case "image": {
      const image = props.token as Tokens.Image;
      return (
        <LinkOut
          href={image.href}
          title={image.title ?? undefined}
          tokens={[{ type: "text", raw: image.text, text: `🖼 ${image.text || image.href}` }]}
        />
      );
    }

    case "checkbox":
      return <BlockToken token={props.token} />;

    default:
      return <RawText token={props.token} />;
  }
}

/** Tokens with no renderer, mostly `html`: shown as raw source, since a swallowed token loses answer text. */
function RawText(props: { token: Token }) {
  const token = props.token as Tokens.Generic;
  return <span class="whitespace-pre-wrap">{token.raw ?? ""}</span>;
}

/** These three schemes only; see `LinkOut`. */
const SCHEMES = new Set(["http:", "https:", "mailto:"]);

/** A link in an answer: `target="_blank"` and an absolute allowlisted scheme, so this window never navigates away; anything else renders as plain text. */
function LinkOut(props: { href: string; title?: string; tokens: Token[] }) {
  const href = createMemo(() => {
    try {
      const url = new URL(props.href);
      return SCHEMES.has(url.protocol) ? url.href : null;
    } catch {
      return null;
    }
  });

  return (
    <Show when={href()} fallback={<InlineSeq tokens={props.tokens} />}>
      {(safe) => (
        <a
          href={safe()}
          title={props.title}
          target="_blank"
          rel="noreferrer noopener"
          class="text-accent-ink underline decoration-line-strong underline-offset-2 hover:decoration-current"
        >
          <InlineSeq tokens={props.tokens} />
        </a>
      )}
    </Show>
  );
}
