import { clearToolRegistry, registerToolCard, registerToolFallback } from "../../lib/registry";
import BashCard from "./BashCard";
import MutationCard from "./MutationCard";
import ReadCard from "./ReadCard";
import { GlobCard, GrepCard } from "./SearchCard";
import GenericToolCard from "./ToolCard";
import TodoToolCard from "./TodoToolCard";

/** Tool-layer extension point, keyed by wire name; anything unkeyed falls back to the generic card. */
registerToolFallback(GenericToolCard);

registerToolCard("read", ReadCard);
registerToolCard("write", MutationCard);
registerToolCard("edit", MutationCard);
registerToolCard("grep", GrepCard);
registerToolCard("glob", GlobCard);
registerToolCard("bash", BashCard);
registerToolCard("todo_write", TodoToolCard);

// See `clearToolRegistry`: hot reload re-runs this file but keeps the registry.
if (import.meta.hot) import.meta.hot.dispose(() => clearToolRegistry());
