import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { StateSnapshot } from "./types";
import { emptySnapshot } from "./types";

export function useActivity() {
  const [snapshot, setSnapshot] = useState<StateSnapshot>(emptySnapshot);
  const [connected, setConnected] = useState(false);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const next = await invoke<StateSnapshot>("get_state");
      setSnapshot(next);
      setConnected(true);
    } catch {
      setConnected(false);
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void refresh();
    const unlisten = listen<StateSnapshot>("activity://state", (event) => {
      setSnapshot(event.payload);
      setConnected(true);
    }).catch(() => {
      setConnected(false);
      return () => {};
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [refresh]);

  return { snapshot, connected, refresh };
}
