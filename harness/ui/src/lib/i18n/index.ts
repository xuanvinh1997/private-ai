export {
  locale,
  setLocale,
  initLocale,
  t,
  tn,
  LOCALES,
  LOCALE_NAMES,
  type Locale,
  type Msg,
} from "./core";
export { TRich } from "./rich";

import { common } from "./strings/common";
import { app } from "./strings/app";
import { chat } from "./strings/chat";
import { providers } from "./strings/providers";
import { embedding } from "./strings/embedding";
import { vision } from "./strings/vision";
import { speech } from "./strings/speech";
import { settings } from "./strings/settings";
import { mcp } from "./strings/mcp";
import { projects } from "./strings/projects";
import { docs } from "./strings/docs";
import { tools } from "./strings/tools";
import { libs } from "./strings/libs";

/** Every app string as one tree behind a single import, split by screen area rather than by kind of text. */
export const S = {
  common,
  app,
  chat,
  providers,
  embedding,
  vision,
  speech,
  settings,
  mcp,
  projects,
  docs,
  tools,
  libs,
} as const;
