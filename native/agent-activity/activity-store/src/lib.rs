use std::path::Path;

use activity_core::ActivityEngine;
use activity_protocol::ActivityEvent;
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};

const DATABASE_VERSION: i64 = 3;

pub struct ActivityStore {
    connection: Connection,
}

impl ActivityStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        initialize_schema(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO app_settings(key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn append_event(&self, event: &ActivityEvent) -> Result<bool> {
        append_event(&self.connection, event)
    }

    pub fn save_event_and_engine(
        &mut self,
        event: &ActivityEvent,
        engine: &ActivityEngine,
    ) -> Result<bool> {
        let transaction = self.connection.transaction()?;
        let inserted = append_event(&transaction, event)?;
        save_engine(&transaction, engine)?;
        transaction.commit()?;
        Ok(inserted)
    }

    pub fn save_engine(&self, engine: &ActivityEngine) -> Result<()> {
        save_engine(&self.connection, engine)
    }

    pub fn load_engine(&self) -> Result<Option<ActivityEngine>> {
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM state_snapshots WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|json| serde_json::from_str(&json).context("invalid state snapshot"))
            .transpose()
    }

    pub fn prune(&self, max_events: usize) -> Result<usize> {
        let cutoff = (Utc::now() - Duration::days(7)).to_rfc3339();
        let expired = self.connection.execute(
            "DELETE FROM activity_events WHERE occurred_at < ?1",
            [cutoff],
        )?;
        let overflow = self.connection.execute(
            "DELETE FROM activity_events WHERE rowid IN (
                SELECT rowid FROM activity_events
                ORDER BY occurred_at DESC LIMIT -1 OFFSET ?1
             )",
            [max_events],
        )?;
        Ok(expired + overflow)
    }
}

fn initialize_schema(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    anyhow::ensure!(
        version <= DATABASE_VERSION,
        "database version {version} is newer than supported version {DATABASE_VERSION}"
    );

    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS state_snapshots (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            payload TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        ",
    )?;

    if version == 0 {
        create_event_table(connection)?;
        connection.pragma_update(None, "user_version", DATABASE_VERSION)?;
    } else if version < DATABASE_VERSION {
        migrate_event_table_v3(connection)?;
    } else {
        create_event_table(connection)?;
    }
    Ok(())
}

fn create_event_table(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
            CREATE TABLE IF NOT EXISTS activity_events (
                provider TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY(provider, instance_id, event_id)
            );
            CREATE INDEX IF NOT EXISTS ix_activity_events_occurred
                ON activity_events(occurred_at);
        ",
    )?;
    Ok(())
}

fn migrate_event_table_v3(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "
        DROP INDEX IF EXISTS ix_activity_events_occurred;
        ALTER TABLE activity_events RENAME TO activity_events_v2;
        CREATE TABLE activity_events (
            provider TEXT NOT NULL,
            instance_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            payload TEXT NOT NULL,
            PRIMARY KEY(provider, instance_id, event_id)
        );
        ",
    )?;

    let legacy_rows = {
        let mut statement = transaction.prepare(
            "SELECT event_id, provider, session_id, kind, occurred_at, payload
             FROM activity_events_v2",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    for (event_id, provider, session_id, kind, occurred_at, payload) in legacy_rows {
        let instance_id = serde_json::from_str::<ActivityEvent>(&payload)
            .map(|event| event.instance_id)
            .unwrap_or_else(|_| "legacy".into());
        transaction.execute(
            "INSERT OR IGNORE INTO activity_events
             (provider, instance_id, event_id, session_id, kind, occurred_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                provider,
                instance_id,
                event_id,
                session_id,
                kind,
                occurred_at,
                payload
            ],
        )?;
    }

    transaction.execute_batch(
        "
        DROP TABLE activity_events_v2;
        CREATE INDEX ix_activity_events_occurred ON activity_events(occurred_at);
        PRAGMA user_version = 3;
        ",
    )?;
    transaction.commit()?;
    Ok(())
}

fn append_event(connection: &Connection, event: &ActivityEvent) -> Result<bool> {
    let payload = serde_json::to_string(event)?;
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO activity_events
         (provider, instance_id, event_id, session_id, kind, occurred_at, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.provider,
            event.instance_id,
            event.event_id,
            event.session_id,
            format!("{:?}", event.kind),
            event.occurred_at.to_rfc3339(),
            payload
        ],
    )?;
    Ok(inserted == 1)
}

fn save_engine(connection: &Connection, engine: &ActivityEngine) -> Result<()> {
    let payload = serde_json::to_string(engine)?;
    connection.execute(
        "INSERT INTO state_snapshots(singleton, payload, updated_at)
         VALUES (1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload, updated_at=excluded.updated_at",
        params![payload, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use activity_protocol::{EventKind, SourceKind, SCHEMA_VERSION};

    use super::*;

    #[test]
    fn saves_and_restores_empty_engine() {
        let store = ActivityStore::open(":memory:").unwrap();
        let engine = ActivityEngine::new();
        store.save_engine(&engine).unwrap();
        assert!(store.load_engine().unwrap().is_some());
    }

    #[test]
    fn event_identity_includes_provider_instance() {
        let store = ActivityStore::open(":memory:").unwrap();
        let first = event("codex", "local", "shared-id");
        let second = event("codex", "remote", "shared-id");
        assert!(store.append_event(&first).unwrap());
        assert!(store.append_event(&second).unwrap());
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn event_and_snapshot_commit_together() {
        let mut store = ActivityStore::open(":memory:").unwrap();
        let event = event("codex", "local", "event-1");
        let mut engine = ActivityEngine::new();
        engine.apply(event.clone()).unwrap();
        assert!(store.save_event_and_engine(&event, &engine).unwrap());
        assert!(store.load_engine().unwrap().is_some());
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrates_v2_event_identity_from_payload() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE activity_events (
                    event_id TEXT PRIMARY KEY,
                    provider TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    occurred_at TEXT NOT NULL,
                    payload TEXT NOT NULL
                );
                PRAGMA user_version = 2;
                ",
            )
            .unwrap();
        let event = event("codex", "desktop-a", "event-1");
        connection
            .execute(
                "INSERT INTO activity_events
                 (event_id, provider, session_id, kind, occurred_at, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.event_id,
                    event.provider,
                    event.session_id,
                    format!("{:?}", event.kind),
                    event.occurred_at.to_rfc3339(),
                    serde_json::to_string(&event).unwrap()
                ],
            )
            .unwrap();

        initialize_schema(&mut connection).unwrap();

        let instance_id: String = connection
            .query_row("SELECT instance_id FROM activity_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(instance_id, "desktop-a");
        assert_eq!(version, DATABASE_VERSION);
    }

    fn event(provider: &str, instance_id: &str, event_id: &str) -> ActivityEvent {
        let now = Utc::now();
        ActivityEvent {
            schema_version: SCHEMA_VERSION.into(),
            event_id: event_id.into(),
            provider: provider.into(),
            adapter_id: format!("builtin.{provider}"),
            adapter_version: "0.1.0".into(),
            source_kind: SourceKind::NativeHook,
            instance_id: instance_id.into(),
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
