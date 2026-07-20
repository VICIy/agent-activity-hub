import { LogicalSize } from "@tauri-apps/api/window";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { floatingStatusLabelKey, statusLabelKey } from "./status";
import type { SessionState, SessionStatus } from "./types";
import { useLocale } from "./i18n";
import { floatingPanelSessions, summarizeActiveProviders } from "./floatingSessions";
import { dismissSession, isDismissibleSessionStatus } from "./sessionActions";
import {
  clampLedBrightness,
  DEFAULT_LED_BRIGHTNESS,
  DEFAULT_LED_MAPPING,
  resolveStatusLedEffect,
  type LedDisplaySettings,
  type LedMapping,
  type LedSettings,
} from "./ledEffects";
import type { FloatingLightOrientation } from "./floatingLightPreferences";

export type Orientation = FloatingLightOrientation;
export const TRAFFIC_LIGHT_COLOR_ORDER = ["green", "amber", "red"] as const;

const lampLabelKeys = {
  green: "status.working",
  amber: "status.waiting_approval",
  red: "status.error",
} as const;

interface Props {
  status: SessionStatus;
  provider: string | null;
  compact?: boolean;
  orientation?: Orientation;
  sessions?: SessionState[];
  expanded?: boolean;
  onToggleExpanded?: () => void;
  onAgentStripHeightChange?: (height: number) => void;
}

export function TrafficLight({
  status,
  provider,
  compact = false,
  orientation = "vertical",
  sessions = [],
  expanded = false,
  onToggleExpanded,
  onAgentStripHeightChange,
}: Props) {
  const { t } = useLocale();
  const [mapping, setMapping] = useState<LedMapping>(DEFAULT_LED_MAPPING);
  const [brightness, setBrightness] = useState(DEFAULT_LED_BRIGHTNESS);
  const agentStripRowRef = useRef<HTMLDivElement>(null);
  const effect = resolveStatusLedEffect(status, mapping);
  const lamps = {
    green: effect.bits[0],
    amber: effect.bits[1],
    red: effect.bits[2],
  };
  const activeProviders = summarizeActiveProviders(sessions);
  const panelSessions = floatingPanelSessions(sessions);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let stopListening: (() => void) | undefined;

    void invoke<LedSettings>("get_led_settings")
      .then((settings) => {
        if (!disposed) {
          setMapping(settings.mapping);
          setBrightness(clampLedBrightness(settings.brightness));
        }
      })
      .catch(() => {});
    void listen<LedDisplaySettings>("led://settings", (event) => {
      setMapping(event.payload.mapping);
      setBrightness(clampLedBrightness(event.payload.brightness));
    }).then((dispose) => {
      if (disposed) dispose();
      else stopListening = dispose;
    }).catch(() => {});

    return () => {
      disposed = true;
      stopListening?.();
    };
  }, []);

  useEffect(() => {
    if (!compact || !onAgentStripHeightChange) return;
    const row = agentStripRowRef.current;
    if (!row) return;

    const reportHeight = () => {
      onAgentStripHeightChange(Math.ceil(row.getBoundingClientRect().height));
    };
    reportHeight();

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(reportHeight);
    observer.observe(row);
    return () => observer.disconnect();
  }, [compact, onAgentStripHeightChange]);

  const orientClass = orientation === "horizontal" ? "horizontal" : "vertical";
  const activeLampStyle = {
    "--lamp-brightness": `${brightness}%`,
    ...(effect.blink ? { "--lamp-blink-duration": `${effect.period * 2}ms` } : {}),
  } as CSSProperties;

  return (
    <div className={`traffic-shell ${orientClass} ${compact ? "compact" : ""} ${expanded ? "expanded" : ""}`} data-tauri-drag-region>
      <div className="traffic-bezel" key={status} data-tauri-drag-region>
        {TRAFFIC_LIGHT_COLOR_ORDER.map((color) => (
          <div
            key={color}
            className={`lamp ${color} ${lamps[color] ? "active" : ""} ${lamps[color] && effect.blink ? "blinking" : ""}`}
            style={lamps[color] ? activeLampStyle : undefined}
            aria-label={t(lampLabelKeys[color])}
          />
        ))}
      </div>
      {!compact && (
        <div className="traffic-caption">
          <span className={`status-dot status-${status}`} />
          <div>
            <strong>{t(statusLabelKey[status])}</strong>
            <span>{provider ?? t("light.localEdge")}</span>
          </div>
        </div>
      )}
      {compact && (
        <>
          <div className="agent-strip-row" ref={agentStripRowRef}>
            <div className="active-agent-strip" aria-label={t("light.activeAgents")} data-tauri-drag-region>
              {activeProviders.length > 0 ? activeProviders.map((agent) => (
                <span
                  className={`agent-chip status-${agent.status}`}
                  key={agent.provider}
                  title={`${agent.provider} · ${t(statusLabelKey[agent.status])}`}
                >
                  <i />
                  <strong>{agent.provider}</strong>
                  {agent.sessionCount > 1 && <small>{agent.sessionCount}</small>}
                </span>
              )) : (
                <span className="agent-chip status-idle"><i />{t("light.noActiveAgents")}</span>
              )}
            </div>
            {onToggleExpanded && (
              <button
                type="button"
                className="panel-toggle"
                title={expanded ? t("light.collapseSessions") : t("light.expandSessions")}
                aria-expanded={expanded}
                onClick={onToggleExpanded}
              >
                {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                <small>{panelSessions.length}</small>
              </button>
            )}
          </div>

          {expanded && (
            <section className="light-session-panel" aria-label={t("light.sessions")}>
              <header><span>{t("light.sessions")}</span><b>{panelSessions.length}</b></header>
              <div className="light-session-list">
                {panelSessions.length > 0 ? panelSessions.map((session) => {
                  const dismissible = isDismissibleSessionStatus(session.status);
                  const dismissLabel = session.status === "error"
                    ? t("session.dismissError")
                    : session.status === "offline"
                      ? t("session.dismissOffline")
                      : session.status === "idle"
                        ? t("session.dismissIdle")
                        : t("session.dismissActive");
                  return (
                    <div
                      className={`light-session-card status-${session.status} ${dismissible ? "has-dismiss" : ""}`}
                      key={`${session.key.provider}:${session.key.instance_id}:${session.key.session_id}`}
                      title={`${session.key.provider} ${session.project ?? t("light.unknownProject")} · ${session.key.session_id}`}
                    >
                      <div className="session-agent-project">
                        <div className="session-agent-row">
                          <i className="session-status-indicator" aria-hidden="true" />
                          <strong>{session.key.provider}</strong>
                          <small>{t(floatingStatusLabelKey[session.status])}</small>
                        </div>
                        <span>{session.project ?? t("light.unknownProject")}</span>
                      </div>
                      {dismissible && (
                        <button
                          type="button"
                          className="session-dismiss"
                          title={dismissLabel}
                          aria-label={dismissLabel}
                          onClick={() => void dismissSession(session.key)}
                        >
                          <X size={8} />
                        </button>
                      )}
                    </div>
                  );
                }) : (
                  <div className="light-session-empty">{t("light.noSessions")}</div>
                )}
              </div>
            </section>
          )}
        </>
      )}
    </div>
  );
}

export const VERTICAL_LIGHT_SIZE = new LogicalSize(112, 222);
export const HORIZONTAL_LIGHT_SIZE = new LogicalSize(184, 130);
export const VERTICAL_LIGHT_EXPANDED_SIZE = new LogicalSize(112, 466);
export const HORIZONTAL_LIGHT_EXPANDED_SIZE = new LogicalSize(184, 374);
export const AGENT_STRIP_BASE_HEIGHT = 32;

export function floatingLightSize(
  orientation: Orientation,
  expanded: boolean,
  agentStripHeight = AGENT_STRIP_BASE_HEIGHT,
): LogicalSize {
  const baseSize = orientation === "horizontal"
    ? (expanded ? HORIZONTAL_LIGHT_EXPANDED_SIZE : HORIZONTAL_LIGHT_SIZE)
    : (expanded ? VERTICAL_LIGHT_EXPANDED_SIZE : VERTICAL_LIGHT_SIZE);
  const wrappedRowsHeight = Math.max(0, Math.ceil(agentStripHeight) - AGENT_STRIP_BASE_HEIGHT);
  return new LogicalSize(baseSize.width, baseSize.height + wrappedRowsHeight);
}
