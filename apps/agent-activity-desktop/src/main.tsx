import React, { useCallback, useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import {
  AGENT_STRIP_BASE_HEIGHT,
  TrafficLight,
  floatingLightSize,
} from "./TrafficLight";
import {
  FLOATING_LIGHT_ORIENTATION_EVENT,
  isFloatingLightOrientation,
  readFloatingLightOrientation,
  writeFloatingLightOrientation,
  type FloatingLightOrientation,
} from "./floatingLightPreferences";
import { useActivity } from "./useActivity";
import { LocaleProvider } from "./i18n";
import "./styles.css";

function FloatingLight() {
  const { snapshot } = useActivity();
  const [orientation, setOrientation] = useState<FloatingLightOrientation>(readFloatingLightOrientation);
  const [expanded, setExpanded] = useState(false);
  const [agentStripHeight, setAgentStripHeight] = useState(AGENT_STRIP_BASE_HEIGHT);

  useEffect(() => {
    if (!isTauri()) return;
    void getCurrentWindow().setSize(floatingLightSize(orientation, expanded, agentStripHeight));
  }, [agentStripHeight, expanded, orientation]);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<FloatingLightOrientation>(FLOATING_LIGHT_ORIENTATION_EVENT, (event) => {
      if (!isFloatingLightOrientation(event.payload)) return;
      writeFloatingLightOrientation(event.payload);
      setOrientation(event.payload);
    }).catch(() => () => {});
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  const updateAgentStripHeight = useCallback((height: number) => {
    setAgentStripHeight((current) => current === height ? current : height);
  }, []);

  return (
    <TrafficLight
      compact
      status={snapshot.global.status}
      provider={snapshot.global.provider}
      orientation={orientation}
      sessions={snapshot.sessions}
      expanded={expanded}
      onToggleExpanded={() => setExpanded((value) => !value)}
      onAgentStripHeightChange={updateAgentStripHeight}
    />
  );
}

const floating = new URLSearchParams(window.location.search).get("view") === "light"
  || (isTauri() && getCurrentWindow().label === "traffic-light");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LocaleProvider>{floating ? <FloatingLight /> : <App />}</LocaleProvider>
  </React.StrictMode>,
);
