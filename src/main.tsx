import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { applyTheme } from "./theme";
import DevToolkit from "./DevToolkit";

type SettingsPayload = { theme: string };

invoke<SettingsPayload>("get_settings")
  .then((settings) => applyTheme(settings.theme))
  .catch(() => {});

listen<SettingsPayload>("cue://settings-changed", (event) => {
  applyTheme(event.payload.theme);
});

const root = document.getElementById("root") as HTMLElement;
const label = getCurrentWebviewWindow().label;

const module =
  label === "settings" ? import("./Settings") : import("./App");

module.then(({ default: Page }) => {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <Page />
      <DevToolkit />
    </React.StrictMode>,
  );
});
