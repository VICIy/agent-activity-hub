import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState, type CSSProperties } from "react";
import type { SessionStatus } from "./types";
import { statusLabelKey } from "./status";
import { useLocale } from "./i18n";
import {
  buildEffect,
  clampLedBrightness,
  clampLedPeriod,
  DEFAULT_LED_BRIGHTNESS,
  DEFAULT_LED_PERIOD,
  FALLBACK_STATUSES,
  MAX_LED_BRIGHTNESS,
  MAX_LED_PERIOD,
  MIN_LED_BRIGHTNESS,
  MIN_LED_PERIOD,
  readLedEffect,
  type LedEffect,
  type LedSettings,
} from "./ledEffects";

export {
  buildEffect,
  clampLedBrightness,
  clampLedPeriod,
  DEFAULT_LED_BRIGHTNESS,
  DEFAULT_LED_PERIOD,
  MAX_LED_BRIGHTNESS,
  MAX_LED_PERIOD,
  MIN_LED_BRIGHTNESS,
  MIN_LED_PERIOD,
} from "./ledEffects";

// LED mask convention: mask[0] = green, mask[1] = yellow, mask[2] = red
const LAMPS: ReadonlyArray<{ color: "green" | "yellow" | "red"; index: 0 | 1 | 2 }> = [
  { color: "green", index: 0 },
  { color: "yellow", index: 1 },
  { color: "red", index: 2 },
];

function PeriodInput({ period, disabled, onCommit }: { period: number; disabled: boolean; onCommit: (period: number) => void }) {
  const [draft, setDraft] = useState(String(period));

  useEffect(() => {
    setDraft(String(period));
  }, [period]);

  function updateDraft(value: string) {
    setDraft(value);
    if (value.trim() === "") return;
    const parsed = Number(value);
    if (Number.isInteger(parsed) && parsed >= MIN_LED_PERIOD && parsed <= MAX_LED_PERIOD) {
      onCommit(parsed);
    }
  }

  function finishEditing() {
    const parsed = draft.trim() === "" ? period : Number(draft);
    const normalized = clampLedPeriod(parsed);
    setDraft(String(normalized));
    onCommit(normalized);
  }

  return (
    <input
      type="number"
      className="led-input led-period"
      min={MIN_LED_PERIOD}
      max={MAX_LED_PERIOD}
      step={20}
      value={draft}
      disabled={disabled}
      onChange={(event) => updateDraft(event.target.value)}
      onBlur={finishEditing}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
      }}
    />
  );
}

export function LedSettingsPanel() {
  const { t } = useLocale();
  const [settings, setSettings] = useState<LedSettings | null>(null);
  const [mappingSaved, setMappingSaved] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<LedSettings>("get_led_settings")
      .then((value) => setSettings({ ...value, brightness: clampLedBrightness(value.brightness) }))
      .catch(() => setSettings(null));
  }, []);

  useEffect(() => {
    if (!mappingSaved) return;
    const handle = window.setTimeout(() => setMappingSaved(null), 2400);
    return () => window.clearTimeout(handle);
  }, [mappingSaved]);

  const statuses = (settings?.statuses ?? FALLBACK_STATUSES) as SessionStatus[];

  function updateEffect(status: string, next: LedEffect) {
    setSettings((prev) => {
      if (!prev) return prev;
      return { ...prev, mapping: { ...prev.mapping, effects: { ...prev.mapping.effects, [status]: next } } };
    });
  }

  async function saveMapping() {
    if (!settings || !isTauri()) return;
    try {
      await invoke("set_led_mapping", { mapping: settings.mapping, brightness: settings.brightness });
      setMappingSaved({ ok: true, text: t("led.effect.savedAt", { t: new Date().toLocaleTimeString() }) });
    } catch (error) {
      setMappingSaved({ ok: false, text: t("led.effect.saveFailed") });
      console.warn("save_led_mapping failed", error);
    }
  }

  return (
    <section className="led-settings">
      <div className="section-heading compact">
        <div>
          <h3>{t("led.title")}</h3>
          <p>{t("led.sub")}</p>
        </div>
        <div className="led-save-cluster">
          <label className="led-brightness-control">
            <span>{t("led.effect.brightness")}</span>
            <input
              type="range"
              min={MIN_LED_BRIGHTNESS}
              max={MAX_LED_BRIGHTNESS}
              step={5}
              value={settings?.brightness ?? DEFAULT_LED_BRIGHTNESS}
              disabled={!settings}
              onChange={(event) => {
                const brightness = clampLedBrightness(Number(event.target.value));
                setSettings((prev) => (prev ? { ...prev, brightness } : prev));
              }}
            />
            <output>{settings?.brightness ?? DEFAULT_LED_BRIGHTNESS}%</output>
          </label>
          {mappingSaved && (
            <span className={`led-save-hint ${mappingSaved.ok ? "ok" : "bad"}`}>{mappingSaved.text}</span>
          )}
          <button type="button" className="command-button" onClick={() => void saveMapping()}>
            {t("led.effect.save")}
          </button>
        </div>
      </div>

      <div className="led-mapping-grid">
        {statuses.map((status) => {
          const raw = settings?.mapping.effects[status];
          const parsed = readLedEffect(raw);
          const { bits, blink, period } = parsed;
          const commit = (nextBits: readonly boolean[], nextBlink: boolean, nextPeriod: number) => {
            updateEffect(status, buildEffect(nextBits, nextBlink, nextPeriod));
          };
          const toggleLamp = (index: 0 | 1 | 2) => {
            const next = [...bits] as [boolean, boolean, boolean];
            next[index] = !next[index];
            commit(next, blink, period);
          };
          const anyOn = bits.some(Boolean);
          // `period` is one on/off phase; a complete blink cycle is twice this value.
          const phaseDuration = clampLedPeriod(period);
          return (
            <div className="led-card" key={status}>
              <div className="led-card-head">
                <b>{t(statusLabelKey[status])}</b>
              </div>
              <div className="led-signal">
                {LAMPS.map(({ color, index }) => {
                  const on = bits[index];
                  const animating = blink && on;
                  return (
                    <button
                      key={color}
                      type="button"
                      className={`led-lamp led-lamp-${color} ${on ? "on" : ""} ${animating ? "blinking" : ""}`}
                      style={{
                        "--led-brightness": `${settings?.brightness ?? DEFAULT_LED_BRIGHTNESS}%`,
                        ...(animating ? { animationDuration: `${phaseDuration * 2}ms` } : {}),
                      } as CSSProperties}
                      onClick={() => toggleLamp(index)}
                      aria-label={color}
                      aria-pressed={on}
                    />
                  );
                })}
              </div>
              <div className="led-card-controls">
                <label className="led-blink-toggle">
                  <input
                    type="checkbox"
                    checked={blink}
                    disabled={!anyOn}
                    onChange={(event) => commit(bits, event.target.checked, period)}
                  />
                  <span>{t("led.effect.blink")}</span>
                </label>
                <label className="led-period-wrap">
                  <span>{t("led.effect.period")}</span>
                  <PeriodInput
                    period={period}
                    disabled={!blink}
                    onCommit={(nextPeriod) => commit(bits, blink, nextPeriod)}
                  />
                </label>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
