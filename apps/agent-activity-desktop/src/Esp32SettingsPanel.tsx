import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useLocale } from "./i18n";

interface Esp32Port { name: string; label: string; likely_esp32: boolean }
interface Esp32Status { connected: boolean; port: string | null; error: string | null }

const disconnected: Esp32Status = { connected: false, port: null, error: null };

export function Esp32SettingsPanel() {
  const { t } = useLocale();
  const [ports, setPorts] = useState<Esp32Port[]>([]);
  const [selected, setSelected] = useState("");
  const [status, setStatus] = useState<Esp32Status>(disconnected);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    if (!isTauri()) return;
    const [nextPorts, nextStatus] = await Promise.all([
      invoke<Esp32Port[]>("list_esp32_ports"),
      invoke<Esp32Status>("get_esp32_status"),
    ]);
    setPorts(nextPorts);
    setStatus(nextStatus);
    setSelected((current) => nextStatus.port ?? (nextPorts.some((port) => port.name === current) ? current : nextPorts[0]?.name ?? ""));
  }

  useEffect(() => { void refresh(); }, []);

  async function toggleConnection() {
    if (!isTauri()) return;
    setBusy(true);
    try {
      const next = status.connected
        ? await invoke<Esp32Status>("disconnect_esp32")
        : await invoke<Esp32Status>("connect_esp32", { port: selected });
      setStatus(next);
    } catch (error) {
      setStatus({ connected: false, port: null, error: String(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="esp32-settings">
      <div>
        <h3>{t("esp32.title")}</h3>
        <p>{t("esp32.sub")}</p>
      </div>
      <div className="esp32-controls">
        <select value={selected} disabled={busy || status.connected} onChange={(event) => setSelected(event.target.value)}>
          {ports.length === 0 && <option value="">{t("esp32.noPorts")}</option>}
          {ports.map((port) => <option key={port.name} value={port.name}>{port.label}{port.likely_esp32 ? " ✓" : ""}</option>)}
        </select>
        <button type="button" className="command-button secondary" disabled={busy || status.connected} onClick={() => void refresh()}>{t("esp32.refresh")}</button>
        <button type="button" className="command-button" disabled={busy || (!status.connected && !selected)} onClick={() => void toggleConnection()}>
          {t(status.connected ? "esp32.disconnect" : "esp32.connect")}
        </button>
      </div>
      <small className={status.error ? "esp32-error" : ""}>
        {status.error ?? (status.connected ? t("esp32.connected", { port: status.port ?? "" }) : t("esp32.disconnected"))}
      </small>
    </section>
  );
}
