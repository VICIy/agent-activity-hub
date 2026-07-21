#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod claude_session;
mod codex_session;
mod esp32;
mod hook_config;
mod led;
mod qoder_session;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use activity_core::{ActivityEngine, ApplyOutcome, StateSnapshot};
use activity_ipc::{Endpoint, EventHandler, Response};
use activity_protocol::{ActivityEvent, EventKind, SessionKey, SourceKind, SCHEMA_VERSION};
use activity_store::ActivityStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use directories::ProjectDirs;
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use uuid::Uuid;

use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tokio::io::AsyncWriteExt;

use crate::led::{
    clamp_brightness, LedEffect, LedMapping, BRIGHTNESS_KEY, DEFAULT_BRIGHTNESS, MAPPING_KEY,
    PERIOD_DEFAULT_MIGRATION_KEY,
};

const OFFLINE_DEFAULT_MIGRATION_KEY: &str = "led.offline-default-v1";

struct Runtime {
    engine: Mutex<ActivityEngine>,
    store: Mutex<ActivityStore>,
    app: Mutex<Option<AppHandle>>,
    led_mapping: Mutex<LedMapping>,
    brightness: Mutex<u8>,
    esp32: esp32::Esp32Manager,
}

impl Runtime {
    fn snapshot(&self) -> StateSnapshot {
        self.engine
            .lock()
            .expect("engine lock poisoned")
            .snapshot(Utc::now())
    }

    fn accept(self: &Arc<Self>, event: ActivityEvent) -> Response {
        if let Err(error) = event.validate() {
            return Response::error(format!("invalid_event:{error}"));
        }
        let mut store = self.store.lock().expect("store lock poisoned");
        let mut engine = self.engine.lock().expect("engine lock poisoned");
        let mut next_engine = engine.clone();
        let outcome = match next_engine.apply(event.clone()) {
            Ok(outcome) => outcome,
            Err(error) => return Response::error(format!("reducer_error:{error}")),
        };
        if let Err(error) = store.save_event_and_engine(&event, &next_engine) {
            return Response::error(format!("store_error:{error}"));
        }
        *engine = next_engine;
        let snapshot = engine.snapshot(Utc::now());
        drop(engine);
        drop(store);
        if let Some(app) = self.app.lock().expect("app lock poisoned").as_ref() {
            let _ = app.emit("activity://state", snapshot.clone());
        }
        self.sync_esp32(&snapshot);
        Response::ok(match outcome {
            ApplyOutcome::StateChanged => "state_changed",
            ApplyOutcome::AcceptedNoChange => "accepted",
            ApplyOutcome::Duplicate => "duplicate",
            ApplyOutcome::DuplicateSourceUpgraded => "source_upgraded",
        })
    }

    fn expire_leases(self: &Arc<Self>) {
        let store = self.store.lock().expect("store lock poisoned");
        let mut engine = self.engine.lock().expect("engine lock poisoned");
        let mut next_engine = engine.clone();
        if !next_engine.expire_leases(Utc::now()) {
            return;
        }
        if store.save_engine(&next_engine).is_err() {
            return;
        }
        *engine = next_engine;
        let snapshot = engine.snapshot(Utc::now());
        drop(engine);
        drop(store);
        if let Some(app) = self.app.lock().expect("app lock poisoned").as_ref() {
            let _ = app.emit("activity://state", snapshot.clone());
        }
        self.sync_esp32(&snapshot);
    }

    fn prune_events(&self) {
        let store = self.store.lock().expect("store lock poisoned");
        let _ = store.prune(10_000);
    }

    fn dismiss_session(self: &Arc<Self>, key: &SessionKey) -> Result<bool, String> {
        let store = self.store.lock().expect("store lock poisoned");
        let mut engine = self.engine.lock().expect("engine lock poisoned");
        let mut next_engine = engine.clone();
        if !next_engine.dismiss_session(key, Utc::now()) {
            return Ok(false);
        }
        store
            .save_engine(&next_engine)
            .map_err(|error| format!("store_error:{error}"))?;
        *engine = next_engine;
        let snapshot = engine.snapshot(Utc::now());
        drop(engine);
        drop(store);
        if let Some(app) = self.app.lock().expect("app lock poisoned").as_ref() {
            let _ = app.emit("activity://state", snapshot.clone());
        }
        self.sync_esp32(&snapshot);
        Ok(true)
    }

    fn persist_led_settings(&self) {
        let mapping = self
            .led_mapping
            .lock()
            .expect("led-mapping lock poisoned")
            .clone();
        let brightness = *self.brightness.lock().expect("brightness lock poisoned");
        let store = self.store.lock().expect("store lock poisoned");
        if let Ok(payload) = serde_json::to_string(&mapping) {
            let _ = store.set_setting(MAPPING_KEY, &payload);
        }
        let _ = store.set_setting(BRIGHTNESS_KEY, &brightness.to_string());
    }

    fn sync_esp32(&self, snapshot: &StateSnapshot) {
        let mapping = self.led_mapping.lock().expect("led-mapping lock poisoned");
        let brightness = *self.brightness.lock().expect("brightness lock poisoned");
        let status = serde_json::to_value(&snapshot.global.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "idle".into());
        self.esp32.sync(&status, &mapping, brightness);
    }
}

#[async_trait]
impl EventHandler for LedRuntime {
    async fn handle(&self, event: ActivityEvent) -> Response {
        self.0.accept(event)
    }
}

/// Wraps `Arc<Runtime>` so IPC's `Arc<dyn EventHandler>` requirement is satisfied without
/// forcing every command path to touch the wrapper.
#[derive(Clone)]
struct LedRuntime(Arc<Runtime>);

#[derive(Clone, Serialize)]
struct LedDisplaySettings {
    mapping: LedMapping,
    brightness: u8,
}

#[derive(Serialize)]
struct LedSettings {
    mapping: LedMapping,
    brightness: u8,
    statuses: Vec<String>,
}

#[tauri::command]
fn get_state(runtime: State<'_, Arc<Runtime>>) -> StateSnapshot {
    runtime.snapshot()
}

#[tauri::command]
fn dismiss_session(key: SessionKey, runtime: State<'_, Arc<Runtime>>) -> Result<bool, String> {
    runtime.inner().dismiss_session(&key)
}

#[tauri::command]
fn get_led_settings(runtime: State<'_, Arc<Runtime>>) -> LedSettings {
    let mapping = runtime
        .led_mapping
        .lock()
        .expect("led-mapping lock poisoned")
        .clone();
    let brightness = *runtime.brightness.lock().expect("brightness lock poisoned");
    LedSettings {
        mapping,
        brightness,
        statuses: vec![
            "offline".into(),
            "idle".into(),
            "working".into(),
            "waiting_approval".into(),
            "complete".into(),
            "error".into(),
            "sleeping".into(),
        ],
    }
}

#[tauri::command]
fn set_led_mapping(
    mapping: LedMapping,
    brightness: u8,
    runtime: State<'_, Arc<Runtime>>,
) -> Result<(), String> {
    let brightness = clamp_brightness(brightness);
    let event_settings = LedDisplaySettings {
        mapping: mapping.clone(),
        brightness,
    };
    {
        let mut current = runtime
            .led_mapping
            .lock()
            .expect("led-mapping lock poisoned");
        *current = mapping;
    }
    *runtime.brightness.lock().expect("brightness lock poisoned") = brightness;
    runtime.persist_led_settings();
    if let Some(app) = runtime.app.lock().expect("app lock poisoned").as_ref() {
        let _ = app.emit("led://settings", event_settings);
    }
    runtime.sync_esp32(&runtime.snapshot());
    Ok(())
}

#[tauri::command]
fn list_esp32_ports() -> Vec<esp32::Esp32Port> {
    esp32::available_ports()
}

#[tauri::command]
fn get_esp32_status(runtime: State<'_, Arc<Runtime>>) -> esp32::Esp32Status {
    runtime.esp32.status()
}

#[tauri::command]
fn connect_esp32(port: String, runtime: State<'_, Arc<Runtime>>) -> Result<esp32::Esp32Status, String> {
    runtime.esp32.connect(&port)?;
    runtime.sync_esp32(&runtime.snapshot());
    Ok(runtime.esp32.status())
}

#[tauri::command]
fn disconnect_esp32(runtime: State<'_, Arc<Runtime>>) -> esp32::Esp32Status {
    runtime.esp32.disconnect();
    runtime.esp32.status()
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart(enabled: bool, app: AppHandle) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_adapter_statuses(app: AppHandle) -> Vec<hook_config::AdapterStatus> {
    hook_config::statuses(&app)
}

fn reveal_traffic_light(app: &AppHandle, center: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("traffic-light")
        .ok_or_else(|| "traffic-light window is unavailable".to_string())?;
    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    if center {
        let _ = window.center();
    }
    window.show().map_err(|error| error.to_string())
}

fn reveal_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn show_traffic_light(app: AppHandle) -> Result<(), String> {
    reveal_traffic_light(&app, true)
}

#[tauri::command]
fn configure_adapter(
    provider: String,
    action: String,
    app: AppHandle,
) -> Result<hook_config::AdapterStatus, String> {
    hook_config::configure(&app, &provider, &action).map_err(|error| error.to_string())
}

#[tauri::command]
fn emit_demo_event(kind: String, runtime: State<'_, Arc<Runtime>>) -> Result<(), String> {
    let event_kind = match kind.as_str() {
        "working" => EventKind::ModelWorking,
        "waiting_approval" => EventKind::ApprovalRequired,
        "complete" => EventKind::RunCompleted,
        "error" => EventKind::RunFailed,
        _ => return Err("unsupported demo state".into()),
    };
    let now = Utc::now();
    let correlation_id = (event_kind == EventKind::ApprovalRequired).then(|| "demo-call".into());
    let response = runtime.accept(ActivityEvent {
        schema_version: SCHEMA_VERSION.into(),
        event_id: format!("demo:{}", Uuid::new_v4()),
        provider: "demo".into(),
        adapter_id: "builtin.diagnostics".into(),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        source_kind: SourceKind::NativeHook,
        instance_id: "local".into(),
        session_id: "output-test".into(),
        turn_id: None,
        correlation_id,
        sequence: None,
        kind: event_kind,
        occurred_at: now,
        observed_at: now,
        tool: None,
        attributes: BTreeMap::new(),
    });
    if response.accepted {
        let cleanup_runtime = runtime.inner().clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            let now = Utc::now();
            let _ = cleanup_runtime.accept(ActivityEvent {
                schema_version: SCHEMA_VERSION.into(),
                event_id: format!("demo-cleanup:{}", Uuid::new_v4()),
                provider: "demo".into(),
                adapter_id: "builtin.diagnostics".into(),
                adapter_version: env!("CARGO_PKG_VERSION").into(),
                source_kind: SourceKind::NativeHook,
                instance_id: "local".into(),
                session_id: "output-test".into(),
                turn_id: None,
                correlation_id: None,
                sequence: None,
                kind: EventKind::SessionStopped,
                occurred_at: now,
                observed_at: now,
                tool: None,
                attributes: BTreeMap::new(),
            });
        });
        Ok(())
    } else {
        Err(response.code)
    }
}

fn application_paths() -> Result<(PathBuf, PathBuf)> {
    let dirs = ProjectDirs::from("work", "Effective Work", "Agent Activity Hub")
        .context("cannot resolve application directory")?;
    let data = dirs.data_local_dir();
    Ok((
        data.join("activity.db"),
        data.join("spool/hook-events.jsonl"),
    ))
}

fn initialize_runtime() -> Result<(Arc<Runtime>, PathBuf)> {
    let (database_path, spool_path) = application_paths()?;
    let store = ActivityStore::open(database_path)?;
    let engine = store
        .load_engine()?
        .map(ActivityEngine::restore_verified)
        .unwrap_or_default();
    store.save_engine(&engine)?;
    store.prune(10_000)?;
    let mut mapping = store
        .get_setting(MAPPING_KEY)?
        .and_then(|payload| serde_json::from_str::<LedMapping>(&payload).ok())
        .unwrap_or_else(LedMapping::defaults);
    if store.get_setting(PERIOD_DEFAULT_MIGRATION_KEY)?.is_none() {
        if mapping.migrate_default_blink_periods() {
            store.set_setting(MAPPING_KEY, &serde_json::to_string(&mapping)?)?;
        }
        store.set_setting(PERIOD_DEFAULT_MIGRATION_KEY, "1")?;
    }
    // Older builds persisted offline as all three lamps on. Offline is kept as
    // a per-session diagnostic state, while an empty global view is idle/off.
    if store.get_setting(OFFLINE_DEFAULT_MIGRATION_KEY)?.is_none() {
        if !matches!(mapping.effects.get("offline"), Some(LedEffect::Solid { leds }) if leds == "000")
        {
            mapping
                .effects
                .insert("offline".into(), LedEffect::Solid { leds: "000".into() });
            store.set_setting(MAPPING_KEY, &serde_json::to_string(&mapping)?)?;
        }
        store.set_setting(OFFLINE_DEFAULT_MIGRATION_KEY, "1")?;
    }
    let brightness = store
        .get_setting(BRIGHTNESS_KEY)?
        .and_then(|value| value.parse::<u8>().ok())
        .map(clamp_brightness)
        .unwrap_or(DEFAULT_BRIGHTNESS);
    Ok((
        Arc::new(Runtime {
            engine: Mutex::new(engine),
            store: Mutex::new(store),
            app: Mutex::new(None),
            led_mapping: Mutex::new(mapping),
            brightness: Mutex::new(brightness),
            esp32: esp32::Esp32Manager::default(),
        }),
        spool_path,
    ))
}

async fn drain_spool(source: &Path, retry_path: &Path, runtime: &Arc<Runtime>) {
    let draining = source.with_extension("jsonl.draining");
    if !draining.exists() && (!source.exists() || fs::rename(source, &draining).is_err()) {
        return;
    }
    let Ok(content) = tokio::fs::read_to_string(&draining).await else {
        return;
    };
    let mut retry = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok(event) = serde_json::from_str::<ActivityEvent>(line) {
            let response = runtime.accept(event);
            if !response.accepted && is_retryable_response(&response.code) {
                retry.extend_from_slice(line.as_bytes());
                retry.push(b'\n');
            }
        }
    }
    if !retry.is_empty() && append_spool_retry(retry_path, &retry).await.is_err() {
        return;
    }
    let _ = tokio::fs::remove_file(draining).await;
}

fn is_retryable_response(code: &str) -> bool {
    code.starts_with("store_error:") || code.starts_with("snapshot_error:")
}

async fn append_spool_retry(path: &Path, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(payload).await?;
    file.flush().await?;
    Ok(())
}

async fn maintenance_loop(path: PathBuf, runtime: Arc<Runtime>) {
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut claude_sessions = claude_session::ClaudeSessionWatcher::from_environment();
    let mut codex_sessions = codex_session::CodexSessionWatcher::from_environment();
    let mut qoder_sessions = qoder_session::QoderSessionWatcher::from_environment();
    let mut ticks = 0_u64;
    loop {
        interval.tick().await;
        runtime.expire_leases();
        if ticks % 4 == 0 {
            let previous = path.with_extension("jsonl.previous");
            drain_spool(&previous, &path, &runtime).await;
            drain_spool(&path, &path, &runtime).await;
        }
        if let Some(watcher) = codex_sessions.as_mut() {
            for event in watcher.poll() {
                let _ = runtime.accept(event);
            }
        }
        if let Some(watcher) = claude_sessions.as_mut() {
            for event in watcher.poll() {
                let _ = runtime.accept(event);
            }
        }
        if let Some(watcher) = qoder_sessions.as_mut() {
            for event in watcher.poll() {
                let _ = runtime.accept(event);
            }
        }
        if ticks % 120 == 0 {
            runtime.prune_events();
        }
        ticks = ticks.wrapping_add(1);
    }
}

fn main() {
    let background = std::env::args().any(|argument| argument == "--background");
    let (runtime, spool_path) = initialize_runtime().expect("initialize Agent Activity Hub");
    let managed_runtime = runtime.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if args.iter().any(|argument| argument == "--background") {
                return;
            }
            let _ = reveal_main_window(app);
            let _ = reveal_traffic_light(app, true);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--background")
                .build(),
        )
        .manage(managed_runtime)
        .invoke_handler(tauri::generate_handler![
            get_state,
            dismiss_session,
            get_led_settings,
            set_led_mapping,
            list_esp32_ports,
            get_esp32_status,
            connect_esp32,
            disconnect_esp32,
            get_autostart,
            set_autostart,
            get_adapter_statuses,
            configure_adapter,
            show_traffic_light,
            emit_demo_event
        ])
        .setup(move |app| {
            *runtime.app.lock().expect("app lock poisoned") = Some(app.handle().clone());

            let _ = reveal_traffic_light(app.handle(), true);

            if background {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            let show = MenuItem::with_id(app, "show", "Show Activity Hub", true, None::<&str>)?;
            let light = MenuItem::with_id(app, "light", "Show Traffic Light", true, None::<&str>)?;
            let unlock =
                MenuItem::with_id(app, "unlock", "Unlock Traffic Light", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &light, &unlock, &quit])?;
            TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .expect("bundled application icon is unavailable")
                        .clone(),
                )
                .tooltip("Agent Activity Hub")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        let _ = reveal_main_window(app);
                    }
                    "light" => {
                        let _ = reveal_traffic_light(app, true);
                    }
                    "unlock" => {
                        if let Some(window) = app.get_webview_window("traffic-light") {
                            let _ = window.set_ignore_cursor_events(false);
                            let _ = app.emit("light://click-through", false);
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let ipc_runtime = LedRuntime(runtime.clone());
            tauri::async_runtime::spawn(async move {
                if let Ok(endpoint) = Endpoint::current_user() {
                    let _ = activity_ipc::serve(endpoint, Arc::new(ipc_runtime)).await;
                }
            });
            let maintenance_runtime = runtime.clone();
            tauri::async_runtime::spawn(maintenance_loop(spool_path, maintenance_runtime));
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("build Agent Activity Hub")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                // The floating light is normally visible, so the main panel may still be hidden.
                let _ = reveal_main_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spool_event_is_applied_and_removed() {
        let runtime = test_runtime();
        let path = std::env::temp_dir().join(format!("agent-activity-{}.jsonl", Uuid::new_v4()));
        let mut payload = serde_json::to_vec(&test_event()).unwrap();
        payload.push(b'\n');
        tokio::fs::write(&path, payload).await.unwrap();

        drain_spool(&path, &path, &runtime).await;

        assert_eq!(runtime.snapshot().accepted_events, 1);
        assert!(!path.exists());
        assert!(!path.with_extension("jsonl.draining").exists());
    }

    #[tokio::test]
    async fn rotated_spool_is_also_applied() {
        let runtime = test_runtime();
        let path = std::env::temp_dir().join(format!("agent-activity-{}.jsonl", Uuid::new_v4()));
        let previous = path.with_extension("jsonl.previous");
        let mut payload = serde_json::to_vec(&test_event()).unwrap();
        payload.push(b'\n');
        tokio::fs::write(&previous, payload).await.unwrap();

        drain_spool(&previous, &path, &runtime).await;

        assert_eq!(runtime.snapshot().accepted_events, 1);
        assert!(!previous.exists());
    }

    #[test]
    fn only_storage_failures_are_retried_from_spool() {
        assert!(is_retryable_response("store_error:database busy"));
        assert!(is_retryable_response("snapshot_error:database busy"));
        assert!(!is_retryable_response("invalid_event:bad provider"));
    }

    fn test_runtime() -> Arc<Runtime> {
        Arc::new(Runtime {
            engine: Mutex::new(ActivityEngine::new()),
            store: Mutex::new(ActivityStore::open(":memory:").unwrap()),
            app: Mutex::new(None),
            led_mapping: Mutex::new(LedMapping::defaults()),
            brightness: Mutex::new(DEFAULT_BRIGHTNESS),
            esp32: esp32::Esp32Manager::default(),
        })
    }

    fn test_event() -> ActivityEvent {
        let now = Utc::now();
        ActivityEvent {
            schema_version: SCHEMA_VERSION.into(),
            event_id: "spooled-event".into(),
            provider: "codex".into(),
            adapter_id: "builtin.codex".into(),
            adapter_version: "0.1.0".into(),
            source_kind: SourceKind::NativeHook,
            instance_id: "local".into(),
            session_id: "session-1".into(),
            turn_id: None,
            correlation_id: None,
            sequence: None,
            kind: EventKind::ModelWorking,
            occurred_at: now,
            observed_at: now,
            tool: None,
            attributes: BTreeMap::new(),
        }
    }
}
