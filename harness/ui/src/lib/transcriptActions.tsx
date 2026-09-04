import { createContext, useContext, type JSX } from "solid-js";

export interface TranscriptActions {
  /** Resend a user message. `null` means a turn is running, so resending is disabled. */
  resend: ((text: string) => void) | null;
  /** Remove a node from the *displayed* transcript. The Rust-side session log is untouched. */
  remove: (id: string) => void;
  /** Open a file in a viewer, at a line when the caller knows one. `null` means paths render as plain text. */
  openFile: ((path: string, line?: number) => void) | null;
}

const NOOP: TranscriptActions = { resend: null, remove: () => {}, openFile: null };

const Ctx = createContext<TranscriptActions>(NOOP);

/** Message actions travel by context, not props, so the renderer contract stays a single `node` prop. */
export function TranscriptActionsProvider(props: {
  value: TranscriptActions;
  children: JSX.Element;
}) {
  return <Ctx.Provider value={props.value}>{props.children}</Ctx.Provider>;
}

export const useTranscriptActions = (): TranscriptActions => useContext(Ctx);
