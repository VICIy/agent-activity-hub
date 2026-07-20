import { createContext, createElement, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

export type Locale = "en" | "zh";

const dict = {
  en: {
    "brand.name": "Agent Activity",
    "brand.tag": "Hub",
    "nav.overview": "Overview",
    "nav.sessions": "Sessions",
    "nav.adapters": "Adapters",
    "nav.diagnostics": "Diagnostics",
    "nav.settings": "Settings",
    "edge.local": "Local edge",
    "edge.connected": "Connected",
    "edge.disconnected": "Not connected",
    "topbar.subtitle": "Agent Activity Hub",
    "topbar.search": "Find session",
    "topbar.refresh": "Refresh state",
    "topbar.showLight": "Show floating light",
    "overview.priority": "Global priority",
    "overview.noSession": "No active session",
    "overview.noProvider": "No provider",
    "overview.revision": "Revision {n}",
    "overview.attention": "Attention",
    "overview.waitingErrors": "{w} waiting · {e} errors",
    "metric.activeSessions": "Active sessions",
    "metric.waiting": "Waiting approval",
    "metric.acceptedEvents": "Accepted events",
    "metric.duplicates": "Duplicates blocked",
    "section.liveSessions": "Live sessions",
    "section.liveSessions.sub": "Ordered by global arbitration priority",
    "section.viewAll": "View all",
    "section.adapters": "Adapters",
    "section.adapters.sub": "Provider inputs",
    "section.allSessions": "All sessions",
    "section.allSessions.sub": "{n} local sessions",
    "section.backOverview": "Back to overview",
    "adapter.codex": "Codex",
    "adapter.codex.detail": "Native hook · L1",
    "adapter.codex.detailFull": "builtin.codex 0.1.0 · Native hook",
    "adapter.claude": "Claude Code",
    "adapter.claude.detail": "Not configured",
    "adapter.claude.detailFull": "Hook adapter is not installed",
    "adapter.qoder": "Qoder",
    "adapter.qoder.detail": "Compatibility spike pending",
    "adapter.qoder.detailFull": "No verified semantic event interface",
    "state.listening": "Listening",
    "state.ready": "Ready",
    "state.available": "Available",
    "state.inferredOnly": "Inferred only",
    "state.healthy": "Healthy",
    "state.install": "Install",
    "state.limited": "Limited",
    "adapters.title": "Provider adapters",
    "adapters.sub": "Detection and hook ownership",
    "adapters.detect": "Detect agents",
    "adapters.detecting": "Detecting Hook configuration...",
    "adapters.desktopOnly": "Hook configuration is available in the desktop application.",
    "adapters.events": "{installed} of {total} lifecycle events installed",
    "adapters.helperUnavailable": "Bundled Hook Helper is unavailable. Reinstall the application.",
    "adapters.install": "Install",
    "adapters.repair": "Repair",
    "adapters.uninstall": "Uninstall managed Hook",
    "adapter.state.installed": "Installed",
    "adapter.state.partial": "Incomplete",
    "adapter.state.legacy": "Legacy Hook",
    "adapter.state.notInstalled": "Not installed",
    "adapter.state.notDetected": "Agent not detected",
    "adapter.state.error": "Configuration error",
    "diag.title": "Runtime diagnostics",
    "diag.sub": "Local, redacted operational data",
    "diag.eventsAccepted": "Events accepted",
    "diag.eventsDeduped": "Events deduplicated",
    "diag.ipc": "IPC endpoint",
    "diag.ready": "Ready",
    "diag.offline": "Offline",
    "diag.testOutput": "Output test",
    "settings.title": "Application settings",
    "settings.sub": "Local edge preferences",
    "settings.launch": "Launch at login",
    "settings.launch.detail": "Start the local edge in background mode",
    "settings.history": "Keep event history",
    "settings.history.detail": "Retain redacted events for 7 days or 10,000 records",
    "settings.language": "Language",
    "settings.language.detail": "Console interface language",
    "settings.language.en": "English",
    "settings.language.zh": "简体中文",
    "settings.lightOrientation": "Floating light orientation",
    "settings.lightOrientation.detail": "Layout of the floating traffic light",
    "settings.lightOrientation.vertical": "Vertical",
    "settings.lightOrientation.horizontal": "Horizontal",
    "table.provider": "Provider / session",
    "table.status": "Status",
    "table.reason": "Reason",
    "table.revision": "Revision",
    "session.dismissError": "Dismiss this error",
    "session.dismissIdle": "Remove this idle session",
    "session.dismissOffline": "Remove this offline session",
    "session.dismissActive": "Remove this active session",
    "empty.noSessions": "No sessions observed",
    "empty.waiting": "Waiting for a provider event",
    "status.offline": "Offline",
    "status.idle": "Idle",
    "status.working": "Working",
    "status.waiting_approval": "Needs approval",
    "status.complete": "Complete",
    "status.error": "Error",
    "status.sleeping": "Sleeping",
    "light.localEdge": "Local edge",
    "light.activeAgents": "Active agents",
    "light.sessions": "Sessions",
    "light.expandSessions": "Expand session list",
    "light.collapseSessions": "Collapse session list",
    "light.noActiveAgents": "Idle",
    "light.noSessions": "No active or idle sessions",
    "light.unknownProject": "Unknown project",
    "light.status.offline": "Offline",
    "light.status.idle": "Idle",
    "light.status.working": "Working",
    "light.status.waiting_approval": "Approval",
    "light.status.complete": "Done",
    "light.status.error": "Error",
    "light.status.sleeping": "Sleeping",
    "led.title": "LED effect mapping",
    "led.sub": "Set the active lamps, brightness, and blink interval for each session status.",
    "led.effect.status": "Status",
    "led.effect.red": "Red",
    "led.effect.yellow": "Yellow",
    "led.effect.green": "Green",
    "led.effect.blink": "Blink",
    "led.effect.period": "Phase interval (ms)",
    "led.effect.brightness": "Brightness",
    "led.effect.save": "Save mapping",
    "led.effect.saveFailed": "Save failed",
    "led.effect.savedAt": "Last saved {t}",
  },
  zh: {
    "brand.name": "Agent 活动",
    "brand.tag": "主控台",
    "nav.overview": "总览",
    "nav.sessions": "会话",
    "nav.adapters": "适配器",
    "nav.diagnostics": "诊断",
    "nav.settings": "设置",
    "edge.local": "本地边缘",
    "edge.connected": "已连接",
    "edge.disconnected": "未连接",
    "topbar.subtitle": "Agent 活动主控台",
    "topbar.search": "查找会话",
    "topbar.refresh": "刷新状态",
    "topbar.showLight": "显示状态浮窗",
    "overview.priority": "全局优先级",
    "overview.noSession": "无活跃会话",
    "overview.noProvider": "无提供方",
    "overview.revision": "版本 {n}",
    "overview.attention": "待关注",
    "overview.waitingErrors": "{w} 待审批 · {e} 错误",
    "metric.activeSessions": "活跃会话",
    "metric.waiting": "等待审批",
    "metric.acceptedEvents": "已接受事件",
    "metric.duplicates": "去重事件",
    "section.liveSessions": "实时会话",
    "section.liveSessions.sub": "按全局仲裁优先级排序",
    "section.viewAll": "查看全部",
    "section.adapters": "适配器",
    "section.adapters.sub": "提供方输入",
    "section.allSessions": "全部会话",
    "section.allSessions.sub": "{n} 个本地会话",
    "section.backOverview": "返回概览",
    "adapter.codex": "Codex",
    "adapter.codex.detail": "原生 Hook · L1",
    "adapter.codex.detailFull": "builtin.codex 0.1.0 · 原生 Hook",
    "adapter.claude": "Claude Code",
    "adapter.claude.detail": "未配置",
    "adapter.claude.detailFull": "Hook 适配器未安装",
    "adapter.qoder": "Qoder",
    "adapter.qoder.detail": "兼容性接入进行中",
    "adapter.qoder.detailFull": "尚未验证的语义事件接口",
    "state.listening": "监听中",
    "state.ready": "就绪",
    "state.available": "可用",
    "state.inferredOnly": "仅推断",
    "state.healthy": "健康",
    "state.install": "安装",
    "state.limited": "受限",
    "adapters.title": "提供方适配器",
    "adapters.sub": "检测与 Hook 归属",
    "adapters.detect": "检测代理",
    "adapters.detecting": "正在检测 Hook 配置...",
    "adapters.desktopOnly": "Hook 配置仅可在桌面应用中使用。",
    "adapters.events": "已安装 {installed}/{total} 个生命周期事件",
    "adapters.helperUnavailable": "应用内置 Hook Helper 不可用，请重新安装应用。",
    "adapters.install": "一键安装",
    "adapters.repair": "修复/重装",
    "adapters.uninstall": "卸载应用管理的 Hook",
    "adapter.state.installed": "已安装",
    "adapter.state.partial": "安装不完整",
    "adapter.state.legacy": "旧版 Hook",
    "adapter.state.notInstalled": "未安装",
    "adapter.state.notDetected": "未检测到 Agent",
    "adapter.state.error": "配置错误",
    "diag.title": "运行时诊断",
    "diag.sub": "本地、脱敏的运行数据",
    "diag.eventsAccepted": "已接受事件",
    "diag.eventsDeduped": "已去重事件",
    "diag.ipc": "IPC 端点",
    "diag.ready": "就绪",
    "diag.offline": "离线",
    "diag.testOutput": "输出测试",
    "settings.title": "应用设置",
    "settings.sub": "本地边缘偏好",
    "settings.launch": "开机自启",
    "settings.launch.detail": "以后台模式启动本地边缘",
    "settings.history": "保留事件历史",
    "settings.history.detail": "保留脱敏事件 7 天或 10,000 条",
    "settings.language": "语言",
    "settings.language.detail": "主控台界面语言",
    "settings.language.en": "English",
    "settings.language.zh": "简体中文",
    "settings.lightOrientation": "浮窗方向",
    "settings.lightOrientation.detail": "设置红绿灯浮窗的横向或纵向布局",
    "settings.lightOrientation.vertical": "纵向",
    "settings.lightOrientation.horizontal": "横向",
    "table.provider": "提供方 / 会话",
    "table.status": "状态",
    "table.reason": "原因",
    "table.revision": "版本",
    "session.dismissError": "关闭此异常",
    "session.dismissIdle": "移除此空闲会话",
    "session.dismissOffline": "移除此离线会话",
    "session.dismissActive": "移除此活动会话",
    "empty.noSessions": "尚未观察到会话",
    "empty.waiting": "等待提供方事件",
    "status.offline": "离线",
    "status.idle": "空闲",
    "status.working": "工作中",
    "status.waiting_approval": "待审批",
    "status.complete": "已完成",
    "status.error": "错误",
    "status.sleeping": "休眠",
    "light.localEdge": "本地边缘",
    "light.activeAgents": "活跃 Agent",
    "light.sessions": "会话",
    "light.expandSessions": "展开会话列表",
    "light.collapseSessions": "收起会话列表",
    "light.noActiveAgents": "空闲",
    "light.noSessions": "暂无活跃或空闲会话",
    "light.unknownProject": "未识别项目",
    "light.status.offline": "离线",
    "light.status.idle": "空闲",
    "light.status.working": "工作中",
    "light.status.waiting_approval": "待审批",
    "light.status.complete": "已完成",
    "light.status.error": "异常",
    "light.status.sleeping": "休眠",
    "led.title": "灯效映射",
    "led.sub": "为每种会话状态设置亮灯组合、亮度和闪烁间隔。",
    "led.effect.status": "状态",
    "led.effect.red": "红",
    "led.effect.yellow": "黄",
    "led.effect.green": "绿",
    "led.effect.blink": "闪烁",
    "led.effect.period": "亮灭间隔 (ms)",
    "led.effect.brightness": "亮度",
    "led.effect.save": "保存映射",
    "led.effect.saveFailed": "保存失败",
    "led.effect.savedAt": "上次保存 {t}",
  },
} as const;

export type TranslationKey = keyof (typeof dict)["en"];

interface LocaleContextValue {
  locale: Locale;
  setLocale: (next: Locale) => void;
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string;
}

const LocaleContext = createContext<LocaleContextValue | null>(null);
const STORAGE_KEY = "agent-activity.locale";
const LOCALE_EVENT = "locale://changed";

function isLocale(value: unknown): value is Locale {
  return value === "en" || value === "zh";
}

function initialLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored === "en" || stored === "zh") return stored;
  } catch {
    /* ignore */
  }
  if (typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("zh")) return "zh";
  return "en";
}

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(initialLocale);

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | undefined;
    const onStorage = (event: StorageEvent) => {
      if (event.key === STORAGE_KEY && isLocale(event.newValue)) setLocaleState(event.newValue);
    };
    window.addEventListener("storage", onStorage);
    if (isTauri()) {
      void listen<Locale>(LOCALE_EVENT, (event) => {
        if (isLocale(event.payload)) setLocaleState(event.payload);
      }).then((dispose) => {
        if (disposed) dispose();
        else stopListening = dispose;
      }).catch(() => {});
    }
    return () => {
      disposed = true;
      stopListening?.();
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  useEffect(() => {
    try {
      window.localStorage.setItem(STORAGE_KEY, locale);
    } catch {
      /* ignore */
    }
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }, [locale]);

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    if (isTauri()) void emit(LOCALE_EVENT, next).catch(() => {});
  }, []);

  const value = useMemo<LocaleContextValue>(() => {
    const table = dict[locale];
    const t = (key: TranslationKey, vars?: Record<string, string | number>) => translate(locale, key, vars);
    return { locale, setLocale, t };
  }, [locale, setLocale]);

  return createElement(LocaleContext.Provider, { value }, children);
}

export function translate(locale: Locale, key: TranslationKey, vars?: Record<string, string | number>): string {
  const table = dict[locale];
  let out: string = table[key] ?? dict.en[key] ?? key;
  if (vars) for (const [name, value] of Object.entries(vars)) out = out.replaceAll(`{${name}}`, String(value));
  return out;
}

export function useLocale(): LocaleContextValue {
  const ctx = useContext(LocaleContext);
  if (!ctx) throw new Error("useLocale must be used inside LocaleProvider");
  return ctx;
}
