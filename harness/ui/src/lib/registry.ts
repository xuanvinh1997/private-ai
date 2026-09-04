import type { Component } from "solid-js";
import type { ConversationNode, NodeKind, ToolCall } from "./protocol";

/** Renderer registries so `Transcript` only knows "key -> component": one keyed by node kind, one by wire tool name. */

export type NodeProps<K extends NodeKind = NodeKind> = {
  node: Extract<ConversationNode, { kind: K }>;
};

const nodeRenderers = new Map<string, Component<NodeProps>>();

export function registerNode<K extends NodeKind>(kind: K, render: Component<NodeProps<K>>): void {
  // A duplicate key is a programming error: which renderer wins would depend on import order.
  if (nodeRenderers.has(kind)) throw new Error(`đã có renderer cho node "${kind}"`);
  // Safe cast: `kind` and the node type are tied in this signature, but the map can only hold the wide type.
  nodeRenderers.set(kind, render as unknown as Component<NodeProps>);
}

export function nodeRenderer(kind: string): Component<NodeProps> | undefined {
  return nodeRenderers.get(kind);
}

export type ToolCardProps = { call: ToolCall };

const toolRenderers = new Map<string, Component<ToolCardProps>>();
let toolFallback: Component<ToolCardProps> | undefined;

export function registerToolCard(name: string, render: Component<ToolCardProps>): void {
  if (toolRenderers.has(name)) throw new Error(`đã có thẻ cho tool "${name}"`);
  toolRenderers.set(name, render);
}

/** Card used when a tool has no renderer of its own; a later registration deliberately overrides an earlier one. */
export function registerToolFallback(render: Component<ToolCardProps>): void {
  toolFallback = render;
}

/** Look up a tool card. The key space is open: an unknown name (say from MCP) falls back instead of throwing. */
export function toolCard(name: string): Component<ToolCardProps> | undefined {
  return toolRenderers.get(name) ?? toolFallback;
}


/** Clear the registries before the registering module re-runs; hot reload would otherwise hit duplicate keys. */
export function clearNodeRegistry(): void {
  nodeRenderers.clear();
}

export function clearToolRegistry(): void {
  toolRenderers.clear();
  toolFallback = undefined;
}
