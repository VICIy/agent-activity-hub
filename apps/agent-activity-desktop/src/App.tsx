import { invoke, isTauri } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import {
  Activity,
  ArrowLeft,
  BellRing,
  Bot,
  Cable,
  CheckCircle2,
  ChevronRight,
  CircleGauge,
  Database,
  Download,
  FlaskConical,
  LoaderCircle,
  PanelTopOpen,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  TerminalSquare,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { statusLabelKey, statusTone } from "./status";
import { TrafficLight } from "./TrafficLight";
import type { AdapterProvider, AdapterStatus, SessionState, SessionStatus } from "./types";
import { useActivity } from "./useActivity";
import { useLocale, type TranslationKey } from "./i18n";
import { LedSettingsPanel } from "./LedSettingsPanel";
import { Esp32SettingsPanel } from "./Esp32SettingsPanel";
import { dismissSession, isDismissibleSessionStatus } from "./sessionActions";
import {
  FLOATING_LIGHT_ORIENTATION_EVENT,
  readFloatingLightOrientation,
  writeFloatingLightOrientation,
  type FloatingLightOrientation,
} from "./floatingLightPreferences";
import {
  adapterCanConfigure,
  adapterNeedsRepair,
  adapterStateLabelKey,
  adapterStateTone,
} from "./adapterStatus";

type NavKey = "overview" | "sessions" | "adapters" | "diagnostics" | "settings";

const nav: ReadonlyArray<readonly [NavKey, TranslationKey, typeof Activity]> = [
  ["overview", "nav.overview", CircleGauge],
  ["sessions", "nav.sessions", TerminalSquare],
  ["adapters", "nav.adapters", Bot],
  ["diagnostics", "nav.diagnostics", Activity],
  ["settings", "nav.settings", Settings],
] as const;

export default function App() {
  const { t } = useLocale();
  const { snapshot, connected, refresh } = useActivity();
  const [view, setView] = useState<NavKey>("overview");
  const [query, setQuery] = useState("");
  const filtered = useMemo(
    () =>
      snapshot.sessions.filter((session) =>
        `${session.key.provider} ${session.key.session_id} ${session.status}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      ),
    [query, snapshot.sessions],
  );
  const waiting = snapshot.sessions.filter((session) => session.status === "waiting_approval").length;
  const errors = snapshot.sessions.filter((session) => session.status === "error").length;
  const currentNav = nav.find(([key]) => key === view)!;

  async function showFloatingLight() {
    if (!isTauri()) return;
    await invoke("show_traffic_light");
  }

  return (
    <div className="app-frame">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><Activity size={18} /></div>
          <div><strong>{t("brand.name")}</strong><span>{t("brand.tag")}</span></div>
        </div>
        <nav>
          {nav.map(([key, labelKey, Icon]) => (
            <button key={key} className={view === key ? "selected" : ""} onClick={() => setView(key)}>
              <Icon size={17} /><span>{t(labelKey)}</span>
              {key === "sessions" && waiting > 0 && <b>{waiting}</b>}
            </button>
          ))}
        </nav>
        <div className="edge-health">
          <span className={connected ? "online" : "offline"} />
          <div><strong>{t("edge.local")}</strong><small>{connected ? t("edge.connected") : t("edge.disconnected")}</small></div>
        </div>
      </aside>

      <main>
        <header className="topbar">
          <div><h1>{t(currentNav[1])}</h1><p>{t("topbar.subtitle")}</p></div>
          <div className="top-actions">
            <label className="search"><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("topbar.search")} /></label>
            <button className="icon-button" title={t("topbar.showLight")} aria-label={t("topbar.showLight")} onClick={() => void showFloatingLight()}><PanelTopOpen size={17} /></button>
            <button className="icon-button" title={t("topbar.refresh")} onClick={() => void refresh()}><RefreshCw size={17} /></button>
          </div>
        </header>

        {view === "overview" && (
          <div className="content overview">
            <section className="status-band">
              <TrafficLight status={snapshot.global.status} provider={snapshot.global.provider} />
              <div className="status-summary">
                <span className={`eyebrow tone-${statusTone[snapshot.global.status]}`}>{t("overview.priority")}</span>
                <h2>{t(statusLabelKey[snapshot.global.status])}</h2>
                <p>{snapshot.global.session_id ?? t("overview.noSession")}</p>
                <div className="summary-meta">
                  <span><Bot size={15} />{snapshot.global.provider ?? t("overview.noProvider")}</span>
                  <span><Database size={15} />{t("overview.revision", { n: snapshot.global.revision })}</span>
                </div>
              </div>
              <div className="attention-count">
                <span>{t("overview.attention")}</span><strong>{waiting + errors}</strong><small>{t("overview.waitingErrors", { w: waiting, e: errors })}</small>
              </div>
            </section>

            <section className="metric-strip">
              <Metric icon={Activity} label={t("metric.activeSessions")} value={String(snapshot.sessions.length)} />
              <Metric icon={BellRing} label={t("metric.waiting")} value={String(waiting)} tone={waiting ? "amber" : undefined} />
              <Metric icon={CheckCircle2} label={t("metric.acceptedEvents")} value={String(snapshot.accepted_events)} />
              <Metric icon={ShieldCheck} label={t("metric.duplicates")} value={String(snapshot.deduplicated_events)} />
            </section>

            <div className="section-heading"><div><h3>{t("section.liveSessions")}</h3><p>{t("section.liveSessions.sub")}</p></div><button className="text-button" onClick={() => setView("sessions")}>{t("section.viewAll")} <ChevronRight size={15} /></button></div>
            <SessionTable sessions={filtered.slice(0, 6)} />

            <div className="bottom-grid">
              <section className="plain-section">
                <div className="section-heading compact"><div><h3>{t("section.adapters")}</h3><p>{t("section.adapters.sub")}</p></div></div>
                <AdapterRow name={t("adapter.codex")} detail={t("adapter.codex.detail")} state={connected ? t("state.listening") : t("state.ready")} icon={TerminalSquare} />
                <AdapterRow name={t("adapter.claude")} detail={t("adapter.claude.detail")} state={t("state.available")} icon={Bot} />
                <AdapterRow name={t("adapter.qoder")} detail={t("adapter.qoder.detail")} state={t("state.inferredOnly")} icon={FlaskConical} />
              </section>
            </div>
          </div>
        )}

        {view === "sessions" && (
          <div className="content">
            <div className="section-heading">
              <div className="section-heading-title">
                <button
                  type="button"
                  className="icon-button section-back"
                  title={t("section.backOverview")}
                  aria-label={t("section.backOverview")}
                  onClick={() => setView("overview")}
                >
                  <ArrowLeft size={17} />
                </button>
                <div><h2>{t("section.allSessions")}</h2><p>{t("section.allSessions.sub", { n: filtered.length })}</p></div>
              </div>
            </div>
            <SessionTable sessions={filtered} />
          </div>
        )}
        {view === "adapters" && <AdaptersView />}
        {view === "diagnostics" && <DiagnosticsView accepted={snapshot.accepted_events} deduped={snapshot.deduplicated_events} connected={connected} />}
        {view === "settings" && <SettingsView />}
      </main>
    </div>
  );
}

function Metric({ icon: Icon, label, value, tone }: { icon: typeof Activity; label: string; value: string; tone?: string }) {
  return <div className={`metric ${tone ? `tone-${tone}` : ""}`}><Icon size={18} /><div><span>{label}</span><strong>{value}</strong></div></div>;
}

function SessionTable({ sessions }: { sessions: SessionState[] }) {
  const { t } = useLocale();
  if (sessions.length === 0) return <div className="empty-state"><Cable size={24} /><strong>{t("empty.noSessions")}</strong><span>{t("empty.waiting")}</span></div>;
  return (
    <div className="session-table">
      <div className="table-head"><span>{t("table.provider")}</span><span>{t("table.status")}</span><span>{t("table.reason")}</span><span>{t("table.revision")}</span></div>
      {sessions.map((session) => {
        const dismissible = isDismissibleSessionStatus(session.status);
        const dismissLabel = session.status === "error"
          ? t("session.dismissError")
          : session.status === "offline"
            ? t("session.dismissOffline")
            : session.status === "idle"
              ? t("session.dismissIdle")
              : t("session.dismissActive");
        return (
          <div className={`table-row ${dismissible ? "has-dismiss" : ""}`} key={`${session.key.provider}:${session.key.instance_id}:${session.key.session_id}`}>
            <div className="session-identity">
              <span className="provider-icon">{session.key.provider.slice(0, 1).toUpperCase()}</span>
              <div>
                <strong>{session.key.session_id}</strong>
                <small className="session-project-name" title={session.project ?? t("light.unknownProject")}>{session.project ?? t("light.unknownProject")}</small>
                <small className="session-source-meta">{session.key.provider} · {session.key.instance_id}</small>
              </div>
            </div>
            <span className={`status-pill tone-${statusTone[session.status]}`}><i />{t(statusLabelKey[session.status])}</span>
            <span className="reason">{session.reason}</span>
            <span className="revision">r{session.revision}</span>
            {dismissible && (
              <button
                type="button"
                className={`session-dismiss-main status-${session.status}`}
                title={dismissLabel}
                aria-label={dismissLabel}
                onClick={() => void dismissSession(session.key)}
              >
                <X size={10} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

function AdapterRow({ name, detail, state, icon: Icon }: { name: string; detail: string; state: string; icon: typeof Activity }) {
  return <div className="adapter-row"><span className="adapter-icon"><Icon size={17} /></span><div><strong>{name}</strong><small>{detail}</small></div><span className="adapter-state">{state}</span></div>;
}

const adapterMetadata: Record<AdapterProvider, { name: TranslationKey; icon: typeof Activity }> = {
  codex: { name: "adapter.codex", icon: TerminalSquare },
  claude: { name: "adapter.claude", icon: Bot },
  qoder: { name: "adapter.qoder", icon: FlaskConical },
};

function AdaptersView() {
  const { t } = useLocale();
  const [statuses, setStatuses] = useState<AdapterStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [activeProvider, setActiveProvider] = useState<AdapterProvider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function detect() {
    if (!isTauri()) {
      setError(t("adapters.desktopOnly"));
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    setNotice(null);
    try {
      setStatuses(await invoke<AdapterStatus[]>("get_adapter_statuses"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void detect();
  }, []);

  async function configure(provider: AdapterProvider, action: "install" | "uninstall") {
    setActiveProvider(provider);
    setError(null);
    setNotice(null);
    try {
      const updated = await invoke<AdapterStatus>("configure_adapter", { provider, action });
      setStatuses((current) => current.map((status) => status.provider === provider ? updated : status));
      if (action === "install") {
        setNotice(t("adapters.restartRequired", { provider: t(adapterMetadata[provider].name) }));
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setActiveProvider(null);
    }
  }

  return (
    <div className="content settings-view">
      <div className="section-heading">
        <div><h2>{t("adapters.title")}</h2><p>{t("adapters.sub")}</p></div>
        <button className="command-button secondary adapter-detect" disabled={loading || activeProvider !== null} onClick={() => void detect()}>
          <RefreshCw className={loading ? "spinning" : ""} size={16} />{t("adapters.detect")}
        </button>
      </div>
      {error && <div className="adapter-error" role="alert">{error}</div>}
      {notice && <div className="adapter-notice" role="status"><RefreshCw size={14} />{notice}</div>}
      <div className="list-panel adapter-list">
        {loading && statuses.length === 0 ? (
          <div className="adapter-loading"><LoaderCircle className="spinning" size={18} />{t("adapters.detecting")}</div>
        ) : statuses.map((status) => (
          <AdapterConfigRow
            key={status.provider}
            status={status}
            busy={activeProvider === status.provider}
            onConfigure={configure}
          />
        ))}
      </div>
    </div>
  );
}

function AdapterConfigRow({ status, busy, onConfigure }: {
  status: AdapterStatus;
  busy: boolean;
  onConfigure: (provider: AdapterProvider, action: "install" | "uninstall") => Promise<void>;
}) {
  const { t } = useLocale();
  const metadata = adapterMetadata[status.provider];
  const Icon = metadata.icon;
  const repair = adapterNeedsRepair(status.state);
  const canConfigure = adapterCanConfigure(status);
  const detail = status.error
    ?? (!status.helper_available ? t("adapters.helperUnavailable") : null)
    ?? t("adapters.events", { installed: status.installed_events, total: status.total_events });
  return (
    <div className="adapter-config-row">
      <span className="adapter-icon"><Icon size={17} /></span>
      <div className="adapter-config-info">
        <div className="adapter-title-line">
          <strong>{t(metadata.name)}</strong>
          <span className={`adapter-status tone-${adapterStateTone(status.state)}`}>{t(adapterStateLabelKey(status.state))}</span>
        </div>
        <small>{detail}</small>
        <small className="adapter-path" title={status.config_path}>{status.config_path}</small>
      </div>
      <div className="adapter-actions">
        <button
          type="button"
          className="command-button adapter-primary-action"
          disabled={busy || !canConfigure}
          onClick={() => void onConfigure(status.provider, "install")}
        >
          {busy ? <LoaderCircle className="spinning" size={14} /> : repair ? <Wrench size={14} /> : <Download size={14} />}
          {t(repair ? "adapters.repair" : "adapters.install")}
        </button>
        {(status.installed_events > 0 || status.legacy_entries > 0) && (
          <button
            type="button"
            className="icon-button adapter-uninstall"
            title={t("adapters.uninstall")}
            aria-label={t("adapters.uninstall")}
            disabled={busy}
            onClick={() => void onConfigure(status.provider, "uninstall")}
          >
            <Trash2 size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

function DiagnosticsView({ accepted, deduped, connected }: { accepted: number; deduped: number; connected: boolean }) {
  const { t } = useLocale();
  async function inject(kind: string) { if (isTauri()) await invoke("emit_demo_event", { kind }); }
  return <div className="content settings-view"><div className="section-heading"><div><h2>{t("diag.title")}</h2><p>{t("diag.sub")}</p></div></div><div className="diagnostic-grid"><Metric icon={Activity} label={t("diag.eventsAccepted")} value={String(accepted)} /><Metric icon={ShieldCheck} label={t("diag.eventsDeduped")} value={String(deduped)} /><Metric icon={Cable} label={t("diag.ipc")} value={connected ? t("diag.ready") : t("diag.offline")} /></div><div className="plain-section test-controls"><h3>{t("diag.testOutput")}</h3><div>{(["working", "waiting_approval", "complete", "error"] as SessionStatus[]).map((kind) => <button key={kind} className="command-button secondary" onClick={() => void inject(kind)}>{t(statusLabelKey[kind])}</button>)}</div></div></div>;
}

function SettingsView() {
  const { t, locale, setLocale } = useLocale();
  return (
    <div className="content settings-view">
      <div className="section-heading"><div><h2>{t("settings.title")}</h2><p>{t("settings.sub")}</p></div></div>
      <div className="list-panel">
        <div className="toggle-row language-row">
          <div><strong>{t("settings.language")}</strong><small>{t("settings.language.detail")}</small></div>
          <div className="language-choices" role="group" aria-label={t("settings.language")}>
            <button type="button" className={`chip ${locale === "en" ? "selected" : ""}`} onClick={() => setLocale("en")}>{t("settings.language.en")}</button>
            <button type="button" className={`chip ${locale === "zh" ? "selected" : ""}`} onClick={() => setLocale("zh")}>{t("settings.language.zh")}</button>
          </div>
        </div>
        <FloatingLightOrientationSetting />
        <AutostartToggle label={t("settings.launch")} detail={t("settings.launch.detail")} />
        <Toggle label={t("settings.history")} detail={t("settings.history.detail")} initial disabled />
      </div>
      <Esp32SettingsPanel />
      <LedSettingsPanel />
    </div>
  );
}

function FloatingLightOrientationSetting() {
  const { t } = useLocale();
  const [orientation, setOrientation] = useState<FloatingLightOrientation>(readFloatingLightOrientation);

  async function update(next: FloatingLightOrientation) {
    setOrientation(next);
    writeFloatingLightOrientation(next);
    if (isTauri()) {
      await emitTo("traffic-light", FLOATING_LIGHT_ORIENTATION_EVENT, next).catch(() => {});
    }
  }

  return (
    <div className="toggle-row orientation-row">
      <div><strong>{t("settings.lightOrientation")}</strong><small>{t("settings.lightOrientation.detail")}</small></div>
      <div className="orientation-choices" role="group" aria-label={t("settings.lightOrientation")}>
        <button type="button" className={orientation === "vertical" ? "selected" : ""} onClick={() => void update("vertical")}>{t("settings.lightOrientation.vertical")}</button>
        <button type="button" className={orientation === "horizontal" ? "selected" : ""} onClick={() => void update("horizontal")}>{t("settings.lightOrientation.horizontal")}</button>
      </div>
    </div>
  );
}

function AutostartToggle({ label, detail }: { label: string; detail: string }) {
  const [enabled, setEnabled] = useState(false);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<boolean>("get_autostart")
      .then((value) => setEnabled(value))
      .catch(() => setEnabled(false))
      .finally(() => setReady(true));
  }, []);

  async function update(next: boolean) {
    if (!isTauri()) return;
    const previous = enabled;
    setEnabled(next);
    try {
      await invoke("set_autostart", { enabled: next });
    } catch {
      setEnabled(previous);
    }
  }

  return <label className="toggle-row"><div><strong>{label}</strong><small>{detail}</small></div><input type="checkbox" checked={enabled} disabled={!ready} onChange={(event) => void update(event.target.checked)} /><span className="toggle-track"><i /></span></label>;
}

function Toggle({ label, detail, initial = false, disabled = false }: { label: string; detail: string; initial?: boolean; disabled?: boolean }) {
  const [enabled, setEnabled] = useState(initial);
  return <label className="toggle-row"><div><strong>{label}</strong><small>{detail}</small></div><input type="checkbox" checked={enabled} disabled={disabled} onChange={(event) => setEnabled(event.target.checked)} /><span className="toggle-track"><i /></span></label>;
}
