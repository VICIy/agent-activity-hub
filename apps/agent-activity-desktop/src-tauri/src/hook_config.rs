use std::{
    env, fs,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use directories::{BaseDirs, ProjectDirs};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const OWNER: &str = "work.effective.agent-activity-hub/v1";
const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SessionEnd",
];
const EXTENDED_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
const QODER_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
    "PreCompact",
    "PostCompact",
    "SubagentStart",
    "SubagentStop",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Codex,
    Claude,
    Qoder,
}

impl Provider {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "qoder" => Ok(Self::Qoder),
            _ => Err(anyhow!("unsupported adapter: {value}")),
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Qoder => "qoder",
        }
    }

    const fn events(self) -> &'static [&'static str] {
        match self {
            Self::Codex => CODEX_EVENTS,
            Self::Claude => EXTENDED_EVENTS,
            Self::Qoder => QODER_EVENTS,
        }
    }

    const fn config_directory(self) -> &'static str {
        match self {
            Self::Codex => ".codex",
            Self::Claude => ".claude",
            Self::Qoder => ".qoder",
        }
    }

    const fn config_file(self) -> &'static str {
        match self {
            Self::Codex => "hooks.json",
            Self::Claude | Self::Qoder => "settings.json",
        }
    }

    const fn executable_names(self) -> &'static [&'static str] {
        match self {
            Self::Codex => &["codex"],
            Self::Claude => &["claude"],
            Self::Qoder => &["qoder", "qodercli"],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterStatus {
    provider: String,
    state: String,
    agent_detected: bool,
    config_exists: bool,
    config_path: String,
    helper_available: bool,
    helper_path: Option<String>,
    installed_events: usize,
    total_events: usize,
    missing_events: Vec<String>,
    legacy_entries: usize,
    error: Option<String>,
}

pub fn statuses(app: &AppHandle) -> Vec<AdapterStatus> {
    let helper = resolve_helper_path(app);
    [Provider::Codex, Provider::Claude, Provider::Qoder]
        .into_iter()
        .map(|provider| inspect(provider, &config_path(provider), helper.as_deref()))
        .collect()
}

pub fn configure(app: &AppHandle, provider: &str, action: &str) -> Result<AdapterStatus> {
    let provider = Provider::parse(provider)?;
    let path = config_path(provider);
    match action {
        "install" => {
            let source = resolve_helper_path(app)
                .ok_or_else(|| anyhow!("bundled Hook Helper is unavailable"))?;
            let helper = persist_helper(&source, &managed_helper_directory()?)?;
            install(provider, &path, &helper)?;
        }
        "uninstall" => uninstall(provider, &path)?,
        _ => return Err(anyhow!("unsupported adapter action: {action}")),
    }
    let helper = resolve_helper_path(app);
    Ok(inspect(provider, &path, helper.as_deref()))
}

fn config_path(provider: Provider) -> PathBuf {
    let override_name = match provider {
        Provider::Codex => "CODEX_HOME",
        Provider::Claude => "CLAUDE_CONFIG_DIR",
        Provider::Qoder => "QODER_CONFIG_DIR",
    };
    if let Some(directory) = env::var_os(override_name).filter(|value| !value.is_empty()) {
        return PathBuf::from(directory).join(provider.config_file());
    }
    let home = BaseDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(provider.config_directory())
        .join(provider.config_file())
}

fn resolve_helper_path(app: &AppHandle) -> Option<PathBuf> {
    let executable_name = if cfg!(windows) {
        "agent-activity-hook.exe"
    } else {
        "agent-activity-hook"
    };
    let mut candidates = Vec::new();
    if let Some(configured) = env::var_os("AGENT_ACTIVITY_HOOK").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(configured));
    }
    if let Ok(current) = env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join(executable_name));
        }
    }
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join(executable_name));
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    candidates.push(workspace.join("target/debug").join(executable_name));
    candidates.push(workspace.join("target/release").join(executable_name));
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn managed_helper_directory() -> Result<PathBuf> {
    let directories = ProjectDirs::from("work", "Effective Work", "Agent Activity Hub")
        .ok_or_else(|| anyhow!("cannot resolve application data directory"))?;
    Ok(directories.data_local_dir().join("hooks"))
}

fn persist_helper(source: &Path, directory: &Path) -> Result<PathBuf> {
    let payload =
        fs::read(source).with_context(|| format!("read Hook Helper {}", source.display()))?;
    let digest = format!("{:x}", Sha256::digest(&payload));
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let destination = directory.join(format!("agent-activity-hook-{}{suffix}", &digest[..16]));

    if destination.is_file() {
        anyhow::ensure!(
            fs::read(&destination)? == payload,
            "installed Hook Helper content does not match its digest: {}",
            destination.display()
        );
        return Ok(destination);
    }

    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".agent-activity-hook-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&payload)?;
    file.sync_all()?;
    drop(file);

    if let Err(error) = fs::rename(&temporary, &destination) {
        if destination.is_file() {
            fs::remove_file(&temporary).ok();
        } else {
            fs::remove_file(&temporary).ok();
            return Err(error)
                .with_context(|| format!("install Hook Helper at {}", destination.display()));
        }
    }
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .ok();
    Ok(destination)
}

fn inspect(provider: Provider, path: &Path, helper: Option<&Path>) -> AdapterStatus {
    let config_exists = path.is_file();
    let agent_detected = path.parent().is_some_and(Path::exists)
        || provider
            .executable_names()
            .iter()
            .any(|name| command_exists(name));
    let base = |state: &str, error: Option<String>| AdapterStatus {
        provider: provider.id().into(),
        state: state.into(),
        agent_detected,
        config_exists,
        config_path: path.to_string_lossy().into_owned(),
        helper_available: helper.is_some_and(Path::is_file),
        helper_path: helper.map(|value| value.to_string_lossy().into_owned()),
        installed_events: 0,
        total_events: provider.events().len(),
        missing_events: provider
            .events()
            .iter()
            .map(|event| (*event).into())
            .collect(),
        legacy_entries: 0,
        error,
    };

    let config = match read_config(path) {
        Ok(config) => config,
        Err(error) => return base("error", Some(error.to_string())),
    };
    let Some(root) = config.as_object() else {
        return base("error", Some("configuration root is not an object".into()));
    };
    let hooks = root.get("hooks").and_then(Value::as_object);
    let legacy_entries = if provider == Provider::Qoder {
        hooks.map_or(0, count_legacy_entries)
    } else {
        0
    };
    let mut installed_events = 0;
    let mut missing_events = Vec::new();
    let mut all_commands_usable = true;
    for event in provider.events() {
        let owned: Vec<_> = hooks
            .and_then(|values| values.get(*event))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|entry| is_owned(entry))
            .collect();
        if owned.is_empty() {
            missing_events.push((*event).to_string());
        } else {
            installed_events += 1;
            all_commands_usable &= owned
                .iter()
                .any(|entry| entry_is_usable(provider, event, entry));
        }
    }
    let state = if legacy_entries > 0 {
        "legacy"
    } else if installed_events == provider.events().len() && all_commands_usable {
        "installed"
    } else if installed_events > 0 {
        "partial"
    } else if !agent_detected {
        "not_detected"
    } else {
        "not_installed"
    };
    AdapterStatus {
        provider: provider.id().into(),
        state: state.into(),
        agent_detected,
        config_exists,
        config_path: path.to_string_lossy().into_owned(),
        helper_available: helper.is_some_and(Path::is_file),
        helper_path: helper.map(|value| value.to_string_lossy().into_owned()),
        installed_events,
        total_events: provider.events().len(),
        missing_events,
        legacy_entries,
        error: (!all_commands_usable && installed_events > 0)
            .then(|| "configured Hook Helper cannot be found".into()),
    }
}

fn install(provider: Provider, path: &Path, helper: &Path) -> Result<()> {
    if !helper.is_file() {
        return Err(anyhow!("Hook Helper does not exist: {}", helper.display()));
    }
    let mut config = read_config(path)?;
    let root = config
        .as_object_mut()
        .ok_or_else(|| anyhow!("configuration root is not an object"))?;
    let hooks = ensure_hooks_object(root)?;

    if provider == Provider::Qoder {
        for value in hooks.values_mut() {
            if let Some(entries) = value.as_array_mut() {
                entries.retain(|entry| !is_legacy_qoder_entry(entry));
            }
        }
    }
    for event in provider.events() {
        let entries = hooks
            .entry((*event).to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| anyhow!("hooks.{event} is not an array"))?;
        entries.retain(|entry| !is_owned(entry));
        entries.push(owned_entry(provider, event, helper));
    }
    hooks.retain(|_, value| !value.as_array().is_some_and(Vec::is_empty));
    write_config(path, &config)
}

fn uninstall(provider: Provider, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut config = read_config(path)?;
    let root = config
        .as_object_mut()
        .ok_or_else(|| anyhow!("configuration root is not an object"))?;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for value in hooks.values_mut() {
            if let Some(entries) = value.as_array_mut() {
                entries.retain(|entry| {
                    !is_owned(entry)
                        && !(provider == Provider::Qoder && is_legacy_qoder_entry(entry))
                });
            }
        }
        hooks.retain(|_, value| !value.as_array().is_some_and(Vec::is_empty));
        if hooks.is_empty() {
            root.remove("hooks");
        }
    }
    write_config(path, &config)
}

fn ensure_hooks_object(root: &mut Map<String, Value>) -> Result<&mut Map<String, Value>> {
    root.entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("the existing hooks field is not an object"))
}

fn owned_entry(provider: Provider, event: &str, helper: &Path) -> Value {
    let helper = helper.to_string_lossy();
    let hook = if provider == Provider::Qoder {
        json!({
            "type": "command",
            "command": helper,
            "args": ["--provider", provider.id(), "--event", event],
            "name": "agent-activity",
            "async": true,
            "timeout": 2
        })
    } else {
        json!({
            "type": "command",
            "command": format!(
                "{} --provider {} --event {}",
                quote_command_path(&helper),
                provider.id(),
                event
            ),
            "timeout": if provider == Provider::Codex { 2 } else { 5 }
        })
    };
    let mut entry = json!({
        "hooks": [hook],
        "x-agent-activity-owner": OWNER,
        "x-agent-activity-event": event
    });
    // A missing Codex matcher means every event/tool. This is also valid for
    // lifecycle events such as PermissionRequest that may not expose a
    // matchable tool name. Claude and Qoder use regex matchers.
    if provider != Provider::Codex {
        entry["matcher"] = json!(".*");
    }
    entry
}

fn quote_command_path(path: &str) -> String {
    if cfg!(windows) {
        format!("& '{}'", path.replace('\'', "''"))
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

fn is_owned(value: &Value) -> bool {
    value.get("x-agent-activity-owner").and_then(Value::as_str) == Some(OWNER)
}

fn is_legacy_qoder_entry(value: &Value) -> bool {
    value
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|hook| {
            hook.get("name").and_then(Value::as_str) == Some("flash4-light")
                || hook
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.ends_with("/flash4-light.sh"))
        })
}

fn count_legacy_entries(hooks: &Map<String, Value>) -> usize {
    hooks
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter(|entry| is_legacy_qoder_entry(entry))
        .count()
}

fn entry_is_usable(provider: Provider, event: &str, entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(Value::as_str))
        .any(|command| {
            command_is_usable(command)
                && (provider != Provider::Codex || command.contains(&format!("--event {event}")))
        })
}

fn command_is_usable(command: &str) -> bool {
    let command = command.trim();
    let command = command
        .strip_prefix('&')
        .map(str::trim_start)
        .unwrap_or(command);
    let executable = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next().unwrap_or_default()
    } else if let Some(rest) = command.strip_prefix('\'') {
        rest.split('\'').next().unwrap_or_default()
    } else {
        command.split_whitespace().next().unwrap_or_default()
    };
    if executable.contains('/') || executable.contains('\\') {
        Path::new(executable).is_file()
    } else {
        command_exists(executable)
    }
}

fn command_exists(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .any(|directory| {
            let candidate = directory.join(command);
            candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
        })
}

fn read_config(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let payload = fs::read_to_string(path)
        .with_context(|| format!("read adapter configuration {}", path.display()))?;
    serde_json::from_str(&payload)
        .with_context(|| format!("parse adapter configuration {}", path.display()))
}

fn write_config(path: &Path, config: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("configuration path has no parent"))?;
    fs::create_dir_all(parent)?;
    if path.exists() {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.json");
        fs::copy(
            path,
            parent.join(format!("{file_name}.agent-activity.{timestamp}.bak")),
        )?;
    }
    let temporary = parent.join(format!(".agent-activity-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, config)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)?;
        } else {
            return Err(error.into());
        }
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qoder_install_replaces_legacy_wrapper_with_direct_helper() {
        let root = test_root();
        let config_path = root.join("settings.json");
        let helper = root.join("agent-activity-hook");
        fs::write(&helper, "helper").unwrap();
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "theme": "dark",
                "hooks": {
                    "PermissionRequest": [{
                        "hooks": [{
                            "command": "/Users/example/.qoder/hooks/flash4-light.sh",
                            "args": ["PermissionRequest"],
                            "name": "flash4-light"
                        }]
                    }],
                    "Notification": [{
                        "hooks": [{
                            "command": "/Users/example/.qoder/hooks/flash4-light.sh",
                            "name": "flash4-light"
                        }]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        install(Provider::Qoder, &config_path, &helper).unwrap();

        let installed = read_config(&config_path).unwrap();
        assert_eq!(installed.get("theme"), Some(&json!("dark")));
        assert!(!installed.to_string().contains("flash4-light"));
        let permission = &installed["hooks"]["PermissionRequest"][0];
        assert!(is_owned(permission));
        assert_eq!(
            permission["hooks"][0]["args"],
            json!(["--provider", "qoder", "--event", "PermissionRequest"])
        );
        assert!(is_owned(&installed["hooks"]["PreCompact"][0]));
        assert_eq!(
            inspect(Provider::Qoder, &config_path, Some(&helper)).state,
            "installed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_and_uninstall_preserve_unowned_configuration() {
        let root = test_root();
        let config_path = root.join("hooks.json");
        let helper = root.join("agent-activity-hook");
        fs::write(&helper, "helper").unwrap();
        let unowned = json!({"matcher":"custom", "hooks":[{"command":"other-hook"}]});
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "model": "example",
                "hooks": {"SessionStart": [unowned.clone()]}
            }))
            .unwrap(),
        )
        .unwrap();

        install(Provider::Codex, &config_path, &helper).unwrap();
        let installed = read_config(&config_path).unwrap();
        assert_eq!(installed["hooks"]["SessionStart"][0], unowned);
        assert_eq!(
            installed["hooks"]["SessionStart"].as_array().unwrap().len(),
            2
        );
        let managed = installed["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| is_owned(entry))
            .unwrap();
        assert!(managed.get("matcher").is_none());
        assert_eq!(
            managed["hooks"][0]["command"],
            json!(format!(
                "{} --provider codex --event SessionStart",
                quote_command_path(&helper.to_string_lossy())
            ))
        );

        uninstall(Provider::Codex, &config_path).unwrap();
        let uninstalled = read_config(&config_path).unwrap();
        assert_eq!(uninstalled["model"], json!("example"));
        assert_eq!(uninstalled["hooks"]["SessionStart"], json!([unowned]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_entries_without_explicit_event_are_not_treated_as_current() {
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        let entry = json!({
            "hooks": [{
                "command": format!("{shell} --provider codex")
            }]
        });
        assert!(!entry_is_usable(
            Provider::Codex,
            "PermissionRequest",
            &entry
        ));
        let current = json!({
            "hooks": [{
                "command": format!("{shell} --provider codex --event PermissionRequest")
            }]
        });
        assert!(entry_is_usable(
            Provider::Codex,
            "PermissionRequest",
            &current
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_commands_use_the_powershell_call_operator() {
        assert_eq!(
            quote_command_path(r"C:\Program Files\Agent Activity\agent-activity-hook.exe"),
            r"& 'C:\Program Files\Agent Activity\agent-activity-hook.exe'"
        );
    }

    #[test]
    fn powershell_command_path_is_detected_as_usable() {
        let root = test_root();
        let helper = root.join("agent activity hook");
        fs::write(&helper, "helper").unwrap();
        let command = format!("& '{}' --provider codex", helper.display());

        assert!(command_is_usable(&command));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn qoder_uninstall_removes_legacy_wrapper_and_preserves_other_hooks() {
        let root = test_root();
        let config_path = root.join("settings.json");
        let custom = json!({"hooks": [{"command": "custom-hook"}]});
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "theme": "dark",
                "hooks": {
                    "PermissionRequest": [{
                        "hooks": [{
                            "command": "/Users/example/.qoder/hooks/flash4-light.sh",
                            "name": "flash4-light"
                        }]
                    }, custom.clone()]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        uninstall(Provider::Qoder, &config_path).unwrap();

        let uninstalled = read_config(&config_path).unwrap();
        assert_eq!(uninstalled["theme"], json!("dark"));
        assert_eq!(uninstalled["hooks"]["PermissionRequest"], json!([custom]));
        assert!(!uninstalled.to_string().contains("flash4-light"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_configuration_is_reported_without_being_replaced() {
        let root = test_root();
        let config_path = root.join("settings.json");
        let helper = root.join("agent-activity-hook");
        fs::write(&helper, "helper").unwrap();
        fs::write(&config_path, "{broken").unwrap();

        assert!(install(Provider::Claude, &config_path, &helper).is_err());
        assert_eq!(fs::read_to_string(&config_path).unwrap(), "{broken");
        assert_eq!(
            inspect(Provider::Claude, &config_path, Some(&helper)).state,
            "error"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_helper_is_content_addressed_and_reused() {
        let root = test_root();
        let source = root.join(if cfg!(windows) {
            "source-helper.exe"
        } else {
            "source-helper"
        });
        let directory = root.join("installed");
        fs::write(&source, "helper-v1").unwrap();

        let first = persist_helper(&source, &directory).unwrap();
        let second = persist_helper(&source, &directory).unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"helper-v1");
        assert_eq!(first.parent(), Some(directory.as_path()));

        fs::write(&source, "helper-v2").unwrap();
        let updated = persist_helper(&source, &directory).unwrap();
        assert_ne!(first, updated);
        assert_eq!(fs::read(updated).unwrap(), b"helper-v2");
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root() -> PathBuf {
        let root = env::temp_dir().join(format!("hook-config-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
