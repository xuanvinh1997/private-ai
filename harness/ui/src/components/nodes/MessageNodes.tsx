import { S, t } from "../../lib/i18n";
import type { NodeProps } from "../../lib/registry";
import { useTranscriptActions } from "../../lib/transcriptActions";
import Blocks from "../markdown/Blocks";
import MessageShell, { type MessageAction } from "../MessageShell";

/** Arrival time: the journal's time when replayed, otherwise now, fixed once per node. */
const arrivedAt = (at?: number) => at ?? Date.now();

async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (err) {
    console.error("could not copy", err);
  }
}

export function UserMessage(props: NodeProps<"user">) {
  const at = arrivedAt(props.node.at);
  const actions = useTranscriptActions();

  const list = (): MessageAction[] => [
    {
      id: "copy",
      label: t(S.tools.message.copy),
      icon: "copy",
      onSelect: () => void copy(props.node.text),
    },
    ...(actions.resend
      ? [
          {
            id: "retry",
            label: t(S.tools.message.resend),
            icon: "retry" as const,
            onSelect: () => actions.resend?.(props.node.text),
          },
        ]
      : []),
    {
      id: "delete",
      label: t(S.tools.message.remove),
      icon: "trash",
      danger: true,
      onSelect: () => actions.remove(props.node.id),
    },
  ];

  return (
    <MessageShell role="user" name={t(S.tools.message.you)} at={at} actions={list()}>
      <div class="text-base whitespace-pre-wrap">{props.node.text}</div>
    </MessageShell>
  );
}

/** Assistant message; `aria-live` must exist before text streams in, and stays polite. */
export function AssistantMessage(props: NodeProps<"assistant">) {
  const at = arrivedAt(props.node.at);
  const actions = useTranscriptActions();

  const list = (): MessageAction[] =>
    props.node.streaming
      ? []
      : [
          {
            id: "copy",
            label: t(S.tools.message.copyReply),
            icon: "copy",
            onSelect: () => void copy(props.node.text),
          },
          {
            id: "delete",
            label: t(S.tools.message.remove),
            icon: "trash",
            danger: true,
            onSelect: () => actions.remove(props.node.id),
          },
        ];

  return (
    <MessageShell
      role="assistant"
      name={t(S.tools.message.assistant)}
      at={at}
      live={props.node.streaming}
      busy={props.node.streaming}
      actions={list()}
    >
      {/* Assistant text goes through the block builder, which also places the caret. */}
      <Blocks text={props.node.text} streaming={props.node.streaming} />
    </MessageShell>
  );
}
