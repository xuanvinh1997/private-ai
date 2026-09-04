/* @refresh reload */
import { render } from "solid-js/web";
import "./styles/app.css";
import App from "./App";
import { initTheme } from "./lib/theme";
import { initLocale } from "./lib/i18n";

// Stamp the theme before the first paint; doing it after render flashes the wrong colours.
initTheme();
initLocale();

const root = document.getElementById("root");
if (!root) throw new Error("thiếu #root trong index.html");
render(() => <App />, root);
