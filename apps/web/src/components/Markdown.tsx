import DOMPurify from "dompurify";
import { Marked } from "marked";
import { createMemo } from "solid-js";

const markdown = new Marked({
  async: false,
  breaks: true,
  gfm: true,
});

export function renderMarkdown(content: string): string {
  const rendered = markdown.parse(content) as string;
  return DOMPurify.sanitize(rendered, {
    FORBID_ATTR: ["style"],
    FORBID_TAGS: ["img", "style"],
    SANITIZE_NAMED_PROPS: true,
    USE_PROFILES: { html: true },
  });
}

export function Markdown(props: { content: string }) {
  const html = createMemo(() => renderMarkdown(props.content));
  return <div class="message-body markdown-body" innerHTML={html()} />;
}
