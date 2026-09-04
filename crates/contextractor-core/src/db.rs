use crate::model::{
    DailyUsage, FileReference, ModelUsage, ParsedSession, ProviderUsage, Role, SessionListItem,
    StoredSession, TokenUsage, ToolCall, ToolUsage, Turn, TurnPage, UsageAnalytics,
};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 11;
const CATALOG_SESSION_FILTER: &str = "EXISTS(SELECT 1 FROM turns visible WHERE visible.session_id=s.id AND visible.role IN ('user','assistant') AND trim(visible.text)<>'') AND (s.provider<>'codex' OR EXISTS(SELECT 1 FROM turns prompt WHERE prompt.session_id=s.id AND prompt.role='user' AND trim(prompt.text)<>'')) AND COALESCE(json_extract(s.metadata_json, '$.session_kind'), 'primary') NOT IN ('subagent','worker') AND COALESCE(CAST(json_extract(s.metadata_json, '$.inherited_fork_snapshot') AS INTEGER), 0)=0";

pub struct Archive {
    connection: Connection,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
        let connection = Connection::open(path)?;
        let mut archive = Self { connection };
        archive.migrate()?;
        Ok(archive)
    }

    pub fn open_existing(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA query_only = ON; PRAGMA busy_timeout = 3000;",
        )?;
        Ok(Self { connection })
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        let mut archive = Self { connection };
        archive.migrate()?;
        Ok(archive)
    }

    fn migrate(&mut self) -> rusqlite::Result<()> {
        self.connection.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS schema_meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS imports (
              source_path TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              source_kind TEXT NOT NULL,
              fingerprint TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              modified_at_ms INTEGER,
              parser_version INTEGER NOT NULL,
              imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              status TEXT NOT NULL,
              error TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              source_kind TEXT NOT NULL,
              external_id TEXT NOT NULL,
              title TEXT NOT NULL,
              project_path TEXT,
              source_path TEXT NOT NULL,
              created_at TEXT,
              updated_at TEXT,
              model TEXT,
              archived INTEGER NOT NULL DEFAULT 0,
              summary TEXT,
              metadata_json TEXT,
              imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              UNIQUE(provider, external_id)
            );

            CREATE TABLE IF NOT EXISTS turns (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
              external_id TEXT,
              ordinal INTEGER NOT NULL,
              role TEXT NOT NULL,
              created_at TEXT,
              text TEXT NOT NULL,
              event_type TEXT,
              model TEXT,
              parent_external_id TEXT,
              metadata_json TEXT,
              UNIQUE(session_id, ordinal)
            );

            CREATE TABLE IF NOT EXISTS tool_calls (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
              turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
              external_id TEXT,
              ordinal INTEGER NOT NULL,
              name TEXT NOT NULL,
              arguments_json TEXT,
              result_text TEXT,
              status TEXT,
              duration_ms INTEGER
            );

            CREATE TABLE IF NOT EXISTS usage_events (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
              turn_id TEXT REFERENCES turns(id) ON DELETE CASCADE,
              input_tokens INTEGER,
              output_tokens INTEGER,
              cached_input_tokens INTEGER,
              cache_write_input_tokens INTEGER,
              reasoning_tokens INTEGER,
              total_tokens INTEGER,
              confidence TEXT NOT NULL,
              source TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_provider_updated
              ON sessions(provider, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_sessions_project
              ON sessions(project_path);
            CREATE INDEX IF NOT EXISTS idx_turns_session_ordinal
              ON turns(session_id, ordinal);
            CREATE INDEX IF NOT EXISTS idx_turns_session_role_ordinal
              ON turns(session_id, role, ordinal);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_session
              ON tool_calls(session_id);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_turn_ordinal
              ON tool_calls(turn_id, ordinal);
            CREATE INDEX IF NOT EXISTS idx_usage_events_session
              ON usage_events(session_id);
            CREATE INDEX IF NOT EXISTS idx_usage_events_turn
              ON usage_events(turn_id);

            DROP TRIGGER IF EXISTS turns_ai;
            DROP TRIGGER IF EXISTS turns_ad;
            DROP TRIGGER IF EXISTS turns_au;
            DROP TRIGGER IF EXISTS turn_search_ai;
            DROP TRIGGER IF EXISTS turn_search_ad;
            DROP TRIGGER IF EXISTS turn_search_au;
            DROP TRIGGER IF EXISTS tool_search_ai;
            DROP TRIGGER IF EXISTS tool_search_ad;
            DROP TRIGGER IF EXISTS tool_search_au;
            DROP TABLE IF EXISTS turns_fts;

            CREATE VIRTUAL TABLE IF NOT EXISTS turn_search USING fts5(
              session_id UNINDEXED,
              turn_id UNINDEXED,
              text,
              tokenize='unicode61 remove_diacritics 2'
            );

            CREATE TRIGGER IF NOT EXISTS turn_search_ai AFTER INSERT ON turns BEGIN
              INSERT INTO turn_search(session_id, turn_id, text)
              VALUES (new.session_id, new.id, substr(new.text, 1, 65536));
            END;
            CREATE TRIGGER IF NOT EXISTS tool_search_ai AFTER INSERT ON tool_calls BEGIN
              INSERT INTO turn_search(session_id, turn_id, text)
              VALUES (new.session_id, new.id, substr(new.name || ' ' || COALESCE(new.arguments_json, '') || ' ' || COALESCE(new.result_text, ''), 1, 65536));
            END;
            "#,
        )?;
        let has_cache_write_column = {
            let mut statement = self.connection.prepare("PRAGMA table_info(usage_events)")?;
            let found = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(Result::ok)
                .any(|name| name == "cache_write_input_tokens");
            found
        };
        if !has_cache_write_column {
            self.connection.execute(
                "ALTER TABLE usage_events ADD COLUMN cache_write_input_tokens INTEGER",
                [],
            )?;
        }
        let previous_version = self
            .connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_default();
        if previous_version < 4 {
            self.connection.execute_batch(
                r#"
                DELETE FROM turn_search;
                INSERT INTO turn_search(session_id, turn_id, text)
                SELECT t.session_id, t.id, substr(t.text, 1, 65536)
                FROM turns t;
                INSERT INTO turn_search(session_id, turn_id, text)
                SELECT tc.session_id, tc.id,
                       substr(tc.name || ' ' || COALESCE(tc.arguments_json, '') || ' ' || COALESCE(tc.result_text, ''), 1, 65536)
                FROM tool_calls tc;
                "#,
            )?;
        }
        self.connection.execute(
            "INSERT INTO schema_meta(key, value) VALUES('schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    pub fn source_is_current(
        &self,
        source_path: &Path,
        fingerprint: &str,
    ) -> rusqlite::Result<bool> {
        self.connection
            .query_row(
                "SELECT fingerprint = ?2 AND status = 'ok' AND parser_version = ?3 FROM imports WHERE source_path = ?1",
                params![source_path.to_string_lossy(), fingerprint, SCHEMA_VERSION],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
    }

    pub fn record_import_failure(
        &mut self,
        source_path: &Path,
        provider: &str,
        source_kind: &str,
        fingerprint: &str,
        size_bytes: u64,
        modified_at_ms: Option<i64>,
        error: &str,
    ) -> rusqlite::Result<()> {
        self.connection.execute(
            r#"
            INSERT INTO imports(
              source_path, provider, source_kind, fingerprint, size_bytes,
              modified_at_ms, parser_version, status, error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'error', ?8)
            ON CONFLICT(source_path) DO UPDATE SET
              provider=excluded.provider,
              source_kind=excluded.source_kind,
              fingerprint=excluded.fingerprint,
              size_bytes=excluded.size_bytes,
              modified_at_ms=excluded.modified_at_ms,
              parser_version=excluded.parser_version,
              imported_at=CURRENT_TIMESTAMP,
              status='error',
              error=excluded.error
            "#,
            params![
                source_path.to_string_lossy(),
                provider,
                source_kind,
                fingerprint,
                size_bytes as i64,
                modified_at_ms,
                SCHEMA_VERSION,
                error,
            ],
        )?;
        Ok(())
    }

    pub fn import_session(
        &mut self,
        parsed: &ParsedSession,
        fingerprint: &str,
        size_bytes: u64,
        modified_at_ms: Option<i64>,
    ) -> rusqlite::Result<String> {
        let transaction = self.connection.transaction()?;
        let session_id = stable_id(&format!(
            "session:{}:{}",
            parsed.provider.as_str(),
            parsed.external_id
        ));
        let has_turns: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE session_id=?1)",
                [&session_id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        let incoming_has_turns = !parsed.turns.is_empty();

        if incoming_has_turns || !has_turns {
            upsert_session_row(&transaction, &session_id, parsed)?;
        } else {
            enrich_session_row(&transaction, &session_id, parsed)?;
        }

        if incoming_has_turns {
            // Deleting FTS rows once per session avoids an O(turns × index-size)
            // cascade when a large transcript is refreshed.
            transaction.execute("DELETE FROM turn_search WHERE session_id=?1", [&session_id])?;
            transaction.execute("DELETE FROM turns WHERE session_id=?1", [&session_id])?;
            for turn in &parsed.turns {
                insert_turn(&transaction, &session_id, turn)?;
            }
        }

        transaction.execute(
            r#"
            INSERT INTO imports(
              source_path, provider, source_kind, fingerprint, size_bytes,
              modified_at_ms, parser_version, status, error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ok', NULL)
            ON CONFLICT(source_path) DO UPDATE SET
              provider=excluded.provider,
              source_kind=excluded.source_kind,
              fingerprint=excluded.fingerprint,
              size_bytes=excluded.size_bytes,
              modified_at_ms=excluded.modified_at_ms,
              parser_version=excluded.parser_version,
              imported_at=CURRENT_TIMESTAMP,
              status='ok',
              error=NULL
            "#,
            params![
                parsed.source_path.to_string_lossy(),
                parsed.provider.as_str(),
                parsed.source_kind.as_str(),
                fingerprint,
                size_bytes as i64,
                modified_at_ms,
                SCHEMA_VERSION,
            ],
        )?;
        transaction.commit()?;
        Ok(session_id)
    }

    pub fn list_sessions(
        &self,
        provider: Option<&str>,
        search: Option<&str>,
        limit: usize,
    ) -> rusqlite::Result<Vec<SessionListItem>> {
        let limit = limit.clamp(1, 10_000) as i64;
        let has_search = search.is_some_and(|query| !query.trim().is_empty());
        let mut sql = if has_search {
            String::from(
                r#"
            WITH matched_sessions AS (
              SELECT DISTINCT session_id FROM turn_search
              WHERE turn_search MATCH ?1 AND instr(lower(text), lower(?2)) > 0
              UNION
              SELECT id FROM sessions
              WHERE instr(lower(title), lower(?2)) > 0
                 OR instr(lower(COALESCE(project_path,'')), lower(?2)) > 0
            )
            SELECT
              s.id, s.provider, s.title, s.project_path, s.source_path, s.created_at, s.updated_at,
              s.model, s.archived,
              COALESCE(
                NULLIF(CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER), 0),
                (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='user')
              ) AS turn_count,
              (SELECT COUNT(*) FROM tool_calls tc WHERE tc.session_id=s.id) AS tool_call_count,
              (SELECT MAX(total_tokens) FROM usage_events u WHERE u.session_id=s.id) AS total_tokens,
              CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER) AS source_turn_count
            FROM sessions s
            JOIN matched_sessions m ON m.session_id=s.id
            "#,
            )
        } else {
            String::from(
                r#"
            SELECT
              s.id, s.provider, s.title, s.project_path, s.source_path, s.created_at, s.updated_at,
              s.model, s.archived,
              COALESCE(
                NULLIF(CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER), 0),
                (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='user')
              ) AS turn_count,
              (SELECT COUNT(*) FROM tool_calls tc WHERE tc.session_id=s.id) AS tool_call_count,
              (SELECT MAX(total_tokens) FROM usage_events u WHERE u.session_id=s.id) AS total_tokens,
              CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER) AS source_turn_count
            FROM sessions s
            "#,
            )
        };
        sql.push_str(" WHERE ");
        sql.push_str(CATALOG_SESSION_FILTER);
        if has_search {
            if provider.is_some() {
                sql.push_str(" AND s.provider=?3");
            }
        } else if provider.is_some() {
            sql.push_str(" AND s.provider=?1");
        }
        sql.push_str(" ORDER BY COALESCE(s.updated_at, s.created_at, s.imported_at) DESC LIMIT ");
        sql.push_str(&limit.to_string());

        let search_query = search.map(fts_query).unwrap_or_default();
        let search_literal = search.map(str::trim).unwrap_or_default();
        let mut statement = self.connection.prepare(&sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(SessionListItem {
                id: row.get(0)?,
                provider: row.get(1)?,
                title: row.get(2)?,
                project_path: row.get(3)?,
                source_path: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                model: row.get(7)?,
                archived: row.get::<_, i64>(8)? != 0,
                turn_count: row.get(9)?,
                tool_call_count: row.get(10)?,
                total_tokens: row.get(11)?,
                source_turn_count: row.get(12)?,
            })
        };
        let rows = match (has_search, provider) {
            (true, Some(provider)) => {
                statement.query_map(params![search_query, search_literal, provider], mapper)?
            }
            (true, None) => statement.query_map(params![search_query, search_literal], mapper)?,
            (false, Some(provider)) => statement.query_map([provider], mapper)?,
            (false, None) => statement.query_map([], mapper)?,
        };
        rows.collect()
    }

    pub fn get_session(&self, id: &str) -> rusqlite::Result<Option<StoredSession>> {
        let Some((session, summary)) = self.get_session_header(id)? else {
            return Ok(None);
        };
        let turns = self.load_turns(id)?;
        Ok(Some(StoredSession {
            session,
            summary,
            turns,
        }))
    }

    pub fn get_session_header(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<(SessionListItem, Option<String>)>> {
        let session = self
            .connection
            .query_row(
                r#"
                SELECT
                  s.id, s.provider, s.title, s.project_path, s.source_path, s.created_at, s.updated_at,
                  s.model, s.archived,
                  COALESCE(
                    NULLIF(CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER), 0),
                    (SELECT COUNT(*) FROM turns WHERE session_id=s.id AND role='user')
                  ),
                  (SELECT COUNT(*) FROM tool_calls WHERE session_id=s.id),
                  (SELECT MAX(total_tokens) FROM usage_events WHERE session_id=s.id),
                  CAST(json_extract(s.metadata_json, '$.source_turn_count') AS INTEGER),
                  s.summary
                FROM sessions s WHERE s.id=?1
                "#,
                [id],
                |row| {
                    Ok((
                        SessionListItem {
                            id: row.get(0)?,
                            provider: row.get(1)?,
                            title: row.get(2)?,
                            project_path: row.get(3)?,
                            source_path: row.get(4)?,
                            created_at: row.get(5)?,
                            updated_at: row.get(6)?,
                            model: row.get(7)?,
                            archived: row.get::<_, i64>(8)? != 0,
                            turn_count: row.get(9)?,
                            tool_call_count: row.get(10)?,
                            total_tokens: row.get(11)?,
                            source_turn_count: row.get(12)?,
                        },
                        row.get::<_, Option<String>>(13)?,
                    ))
                },
            )
            .optional()?;
        Ok(session)
    }

    pub fn session_usage_estimate(&self, id: &str) -> rusqlite::Result<TokenUsage> {
        let observed = self.connection.query_row(
            r#"
            SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                   COALESCE(SUM(cached_input_tokens),0), COALESCE(SUM(cache_write_input_tokens),0), COALESCE(SUM(reasoning_tokens),0),
                   MAX(CASE WHEN confidence='observed' THEN 1 ELSE 0 END)
            FROM usage_events WHERE session_id=?1
            "#,
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                ))
            },
        )?;
        if observed.5 && (observed.0 > 0 || observed.1 > 0) {
            return Ok(TokenUsage {
                input_tokens: Some(observed.0),
                output_tokens: Some(observed.1),
                cached_input_tokens: Some(observed.2),
                cache_write_input_tokens: Some(observed.3),
                reasoning_tokens: Some(observed.4),
                total_tokens: Some(observed.0 + observed.1),
                confidence: Some(crate::model::UsageConfidence::Observed),
                source: Some("provider record".to_string()),
            });
        }
        let estimated = self.connection.query_row(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN role IN ('user','system','tool') THEN length(text) ELSE 0 END),0),
              COALESCE(SUM(CASE WHEN role IN ('assistant','reasoning') THEN length(text) ELSE 0 END),0),
              COALESCE((SELECT SUM(length(COALESCE(arguments_json,'')) + length(COALESCE(result_text,'')))
                        FROM tool_calls WHERE session_id=?1),0)
            FROM turns WHERE session_id=?1
            "#,
            [id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        )?;
        let input = ((estimated.0 + estimated.2) + 3) / 4;
        let output = (estimated.1 + 3) / 4;
        Ok(TokenUsage {
            input_tokens: Some(input),
            output_tokens: Some(output),
            cached_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            reasoning_tokens: Some(0),
            total_tokens: Some(input + output),
            confidence: Some(crate::model::UsageConfidence::Estimated),
            source: Some("character approximation".to_string()),
        })
    }

    pub fn provider_counts(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let sql = format!(
            "SELECT s.provider, COUNT(*) FROM sessions s WHERE {CATALOG_SESSION_FILTER} GROUP BY s.provider ORDER BY s.provider"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let result = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect();
        result
    }

    fn load_turns(&self, session_id: &str) -> rusqlite::Result<Vec<Turn>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT t.id, t.external_id, t.ordinal, t.role, t.created_at, t.text, t.event_type,
                   t.model, t.parent_external_id, t.metadata_json,
                   (SELECT COUNT(*) FROM turns prompt
                    WHERE prompt.session_id=t.session_id AND prompt.role='user'
                      AND prompt.ordinal<=t.ordinal) AS prompt_ordinal
            FROM turns t WHERE t.session_id=?1 ORDER BY t.ordinal
            "#,
        )?;
        let rows = statement.query_map([session_id], |row| {
            let turn_id: String = row.get(0)?;
            Ok((
                turn_id,
                Turn {
                    external_id: row.get(1)?,
                    ordinal: row.get(2)?,
                    prompt_ordinal: row.get(10)?,
                    role: parse_role(&row.get::<_, String>(3)?),
                    created_at: row.get(4)?,
                    text: row.get(5)?,
                    event_type: row.get(6)?,
                    model: row.get(7)?,
                    parent_external_id: row.get(8)?,
                    usage: None,
                    tool_calls: Vec::new(),
                    metadata_json: row.get(9)?,
                },
            ))
        })?;
        let turns = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        self.attach_turn_details(turns, true)
    }

    pub fn load_turn_page(
        &self,
        session_id: &str,
        mode: &str,
        offset: usize,
        limit: usize,
        search: Option<&str>,
    ) -> rusqlite::Result<TurnPage> {
        let filter = match mode {
            "conversation" => " AND t.role IN ('user','assistant') AND trim(t.text)<>''",
            "prompts" => " AND t.role='user' AND trim(t.text)<>''",
            "system" => " AND t.role='system' AND trim(t.text)<>''",
            "responses" => " AND t.role='assistant' AND trim(t.text)<>''",
            "tools" => {
                " AND (t.role='tool' OR EXISTS(SELECT 1 FROM tool_calls tc WHERE tc.turn_id=t.id))"
            }
            "reasoning" => " AND t.role='reasoning' AND trim(t.text)<>''",
            _ => " AND t.role!='reasoning'",
        };
        let has_search = search.is_some_and(|value| !value.trim().is_empty());
        let search_filter = if has_search {
            " AND (instr(lower(t.text), lower(?2)) > 0 OR EXISTS(SELECT 1 FROM tool_calls tc WHERE tc.turn_id=t.id AND (instr(lower(tc.name), lower(?2)) > 0 OR instr(lower(COALESCE(tc.arguments_json,'')), lower(?2)) > 0 OR instr(lower(COALESCE(tc.result_text,'')), lower(?2)) > 0)))"
        } else {
            ""
        };
        let count_sql =
            format!("SELECT COUNT(*) FROM turns t WHERE t.session_id=?1{filter}{search_filter}");
        let total: i64 = if let Some(search) = search.filter(|value| !value.trim().is_empty()) {
            self.connection
                .query_row(&count_sql, params![session_id, search.trim()], |row| {
                    row.get(0)
                })?
        } else {
            self.connection
                .query_row(&count_sql, [session_id], |row| row.get(0))?
        };
        let total = total.max(0) as usize;
        let limit = limit.clamp(1, 200);
        let page_search_filter = if has_search {
            " AND (instr(lower(t.text), lower(?4)) > 0 OR EXISTS(SELECT 1 FROM tool_calls tc WHERE tc.turn_id=t.id AND (instr(lower(tc.name), lower(?4)) > 0 OR instr(lower(COALESCE(tc.arguments_json,'')), lower(?4)) > 0 OR instr(lower(COALESCE(tc.result_text,'')), lower(?4)) > 0)))"
        } else {
            ""
        };
        let sql = format!(
            "SELECT t.id, t.external_id, t.ordinal, t.role, t.created_at, t.text, t.event_type, \
             t.model, t.parent_external_id, t.metadata_json, \
             (SELECT COUNT(*) FROM turns prompt WHERE prompt.session_id=t.session_id \
              AND prompt.role='user' AND prompt.ordinal<=t.ordinal) AS prompt_ordinal FROM turns t \
             WHERE t.session_id=?1{filter}{page_search_filter} ORDER BY t.ordinal LIMIT ?2 OFFSET ?3"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                Turn {
                    external_id: row.get(1)?,
                    ordinal: row.get(2)?,
                    prompt_ordinal: row.get(10)?,
                    role: parse_role(&row.get::<_, String>(3)?),
                    created_at: row.get(4)?,
                    text: row.get(5)?,
                    event_type: row.get(6)?,
                    model: row.get(7)?,
                    parent_external_id: row.get(8)?,
                    usage: None,
                    tool_calls: Vec::new(),
                    metadata_json: row.get(9)?,
                },
            ))
        };
        let rows = if let Some(search) = search.filter(|value| !value.trim().is_empty()) {
            statement.query_map(
                params![session_id, limit as i64, offset as i64, search.trim()],
                mapper,
            )?
        } else {
            statement.query_map(params![session_id, limit as i64, offset as i64], mapper)?
        };
        let turns = self.attach_turn_details(rows.collect::<rusqlite::Result<Vec<_>>>()?, false)?;
        let loaded = offset + turns.len();
        Ok(TurnPage {
            turns,
            offset,
            total,
            has_more: loaded < total,
        })
    }

    fn attach_turn_details(
        &self,
        rows: Vec<(String, Turn)>,
        include_tool_payloads: bool,
    ) -> rusqlite::Result<Vec<Turn>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");

        let mut tools_by_turn: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let payload_columns = if include_tool_payloads {
            "arguments_json, result_text"
        } else {
            "NULL, NULL"
        };
        let tool_sql = format!(
            "SELECT turn_id, external_id, name, {payload_columns}, status, duration_ms \
             FROM tool_calls WHERE turn_id IN ({placeholders}) ORDER BY turn_id, ordinal"
        );
        let mut tool_statement = self.connection.prepare(&tool_sql)?;
        let tool_rows = tool_statement.query_map(params_from_iter(ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                ToolCall {
                    external_id: row.get(1)?,
                    name: row.get(2)?,
                    arguments_json: row.get(3)?,
                    result_text: row.get(4)?,
                    status: row.get(5)?,
                    duration_ms: row.get(6)?,
                },
            ))
        })?;
        for row in tool_rows {
            let (turn_id, tool) = row?;
            tools_by_turn.entry(turn_id).or_default().push(tool);
        }

        let usage_sql = format!(
            "SELECT turn_id, input_tokens, output_tokens, cached_input_tokens, cache_write_input_tokens, reasoning_tokens, \
             total_tokens, confidence, source FROM usage_events WHERE turn_id IN ({placeholders})"
        );
        let mut usage_statement = self.connection.prepare(&usage_sql)?;
        let usage_rows = usage_statement.query_map(params_from_iter(ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                TokenUsage {
                    input_tokens: row.get(1)?,
                    output_tokens: row.get(2)?,
                    cached_input_tokens: row.get(3)?,
                    cache_write_input_tokens: row.get(4)?,
                    reasoning_tokens: row.get(5)?,
                    total_tokens: row.get(6)?,
                    confidence: Some(match row.get::<_, String>(7)?.as_str() {
                        "observed" => crate::model::UsageConfidence::Observed,
                        "reconstructed" => crate::model::UsageConfidence::Reconstructed,
                        _ => crate::model::UsageConfidence::Estimated,
                    }),
                    source: row.get(8)?,
                },
            ))
        })?;
        let mut usage_by_turn = HashMap::new();
        for row in usage_rows {
            let (turn_id, usage) = row?;
            usage_by_turn.entry(turn_id).or_insert(usage);
        }

        Ok(rows
            .into_iter()
            .map(|(id, mut turn)| {
                turn.tool_calls = tools_by_turn.remove(&id).unwrap_or_default();
                turn.usage = usage_by_turn.remove(&id);
                turn
            })
            .collect())
    }

    pub fn load_tool_call(
        &self,
        session_id: &str,
        turn_ordinal: i64,
        tool_ordinal: i64,
    ) -> rusqlite::Result<Option<ToolCall>> {
        self.connection
            .query_row(
                r#"
                SELECT tc.external_id, tc.name, tc.arguments_json, tc.result_text,
                       tc.status, tc.duration_ms
                FROM tool_calls tc
                JOIN turns t ON t.id=tc.turn_id
                WHERE t.session_id=?1 AND t.ordinal=?2 AND tc.ordinal=?3
                "#,
                params![session_id, turn_ordinal, tool_ordinal],
                |row| {
                    Ok(ToolCall {
                        external_id: row.get(0)?,
                        name: row.get(1)?,
                        arguments_json: row.get(2)?,
                        result_text: row.get(3)?,
                        status: row.get(4)?,
                        duration_ms: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    pub fn session_file_references(
        &self,
        session_id: &str,
    ) -> rusqlite::Result<Vec<FileReference>> {
        // Source provenance and the workspace are shown in their own ledger.
        // This list is intentionally limited to paths actually mentioned by a
        // turn so they cannot be mistaken for chat attachments.
        let mut texts = Vec::new();
        let mut statement = self.connection.prepare(
            "SELECT role, text, metadata_json FROM turns WHERE session_id=?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (role, text, metadata) = row?;
            let origin = match role.as_str() {
                "user" => "user",
                "assistant" | "reasoning" => "assistant",
                "tool" => "tool",
                "system" => "system",
                _ => "unknown",
            };
            texts.push((text, origin));
            if let Some(metadata) = metadata {
                texts.push((metadata, origin));
            }
        }

        let mut tool_statement = self.connection.prepare(
            "SELECT tc.arguments_json, tc.result_text FROM tool_calls tc JOIN turns t ON t.id=tc.turn_id WHERE tc.session_id=?1 ORDER BY t.ordinal, tc.ordinal",
        )?;
        let tool_rows = tool_statement.query_map([session_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;
        for row in tool_rows {
            let (arguments, result) = row?;
            if let Some(arguments) = arguments {
                texts.push((arguments, "tool"));
            }
            if let Some(result) = result {
                texts.push((result, "tool"));
            }
        }

        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut paths = Vec::new();
        for (text, origin) in texts {
            for path in extract_file_paths(&text) {
                let key = path.to_ascii_lowercase();
                if let Some(index) = seen.get(&key).copied() {
                    let reference: &mut FileReference = &mut paths[index];
                    if !reference.origins.iter().any(|value| value == origin) {
                        reference.origins.push(origin.to_string());
                    }
                    continue;
                }
                let file_path = std::path::Path::new(&path);
                let extension = file_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let exists = file_path.exists();
                paths.push(FileReference {
                    path,
                    exists,
                    is_image: matches!(
                        extension.as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif"
                    ),
                    origins: vec![origin.to_string()],
                });
                seen.insert(key, paths.len() - 1);
                if paths.len() >= 100 {
                    return Ok(paths);
                }
            }
        }
        Ok(paths)
    }

    /// Export packages intentionally include only explicit @ mentions and
    /// actual attachment metadata, never incidental project/tool paths.
    pub fn session_export_file_references(
        &self,
        session_id: &str,
    ) -> rusqlite::Result<Vec<FileReference>> {
        let mut statement = self.connection.prepare(
            "SELECT role, text, metadata_json FROM turns WHERE session_id=?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut paths: Vec<FileReference> = Vec::new();
        for row in rows {
            let (role, text, metadata) = row?;
            let origin = match role.as_str() {
                "user" => "user",
                "assistant" | "reasoning" => "assistant",
                _ => continue,
            };
            let mut candidates = extract_explicit_file_paths(&text);
            if let Some(metadata) = metadata {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata) {
                    if value.get("attachment_type").is_some()
                        || value.get("filename").is_some()
                        || value.pointer("/content/file/filePath").is_some()
                    {
                        collect_json_paths(&value, &mut candidates);
                    }
                }
            }
            for path in candidates {
                let key = path.to_ascii_lowercase();
                if let Some(index) = seen.get(&key).copied() {
                    if !paths[index].origins.iter().any(|value| value == origin) {
                        paths[index].origins.push(origin.to_string());
                    }
                    continue;
                }
                let file_path = std::path::Path::new(&path);
                let extension = file_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                paths.push(FileReference {
                    exists: file_path.exists(),
                    is_image: matches!(
                        extension.as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif"
                    ),
                    path,
                    origins: vec![origin.to_string()],
                });
                seen.insert(key, paths.len() - 1);
            }
        }
        Ok(paths)
    }

    pub fn usage_analytics(&self, provider: Option<&str>) -> rusqlite::Result<UsageAnalytics> {
        let provider_clause = if provider.is_some() {
            " WHERE provider=?1"
        } else {
            ""
        };
        let provider_sql = format!(
            r#"
            WITH session_usage AS (
              SELECT s.id, s.provider,
                     (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='user') prompts,
                     (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='assistant') assistant_turns,
                     (SELECT COUNT(*) FROM tool_calls tc WHERE tc.session_id=s.id) tool_calls,
                     COALESCE((SELECT MAX(total_tokens) FROM usage_events u WHERE u.session_id=s.id), 0) total_tokens,
                     substr(COALESCE(s.updated_at, s.created_at, s.imported_at), 1, 10) active_date
              FROM sessions s WHERE {CATALOG_SESSION_FILTER}
            )
            SELECT provider, COUNT(*), SUM(prompts), SUM(assistant_turns), SUM(tool_calls),
                   SUM(total_tokens), COUNT(DISTINCT active_date),
                   CAST(SUM(prompts) AS REAL) / MAX(COUNT(*), 1)
            FROM session_usage{provider_clause}
            GROUP BY provider ORDER BY COUNT(*) DESC
            "#
        );
        let mut statement = self.connection.prepare(&provider_sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(ProviderUsage {
                provider: row.get(0)?,
                sessions: row.get(1)?,
                prompts: row.get(2)?,
                assistant_turns: row.get(3)?,
                tool_calls: row.get(4)?,
                total_tokens: row.get(5)?,
                active_days: row.get(6)?,
                average_prompts_per_session: row.get(7)?,
            })
        };
        let providers = if let Some(provider) = provider {
            statement
                .query_map([provider], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            statement
                .query_map([], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let day_filter = if provider.is_some() {
            " AND s.provider=?1"
        } else {
            ""
        };
        let daily_sql = format!(
            r#"
            WITH turn_activity AS (
              SELECT t.id, t.session_id, s.provider,
                     substr(COALESCE(t.created_at, s.updated_at, s.created_at, s.imported_at), 1, 10) day,
                     CASE WHEN t.role='user' THEN 1 ELSE 0 END prompts,
                     CASE WHEN t.role='assistant' THEN 1 ELSE 0 END assistant_turns,
                     (SELECT COUNT(*) FROM tool_calls tc WHERE tc.turn_id=t.id) tool_calls,
                     COALESCE((SELECT total_tokens FROM usage_events u WHERE u.turn_id=t.id LIMIT 1), 0) total_tokens
              FROM turns t JOIN sessions s ON s.id=t.session_id WHERE {CATALOG_SESSION_FILTER}{day_filter}
            )
            SELECT day, provider, COUNT(DISTINCT session_id), SUM(prompts), SUM(assistant_turns), SUM(tool_calls), SUM(total_tokens)
            FROM turn_activity WHERE day IS NOT NULL AND length(day)=10
            GROUP BY day, provider ORDER BY day DESC
            "#
        );
        let mut daily_statement = self.connection.prepare(&daily_sql)?;
        let daily_mapper = |row: &rusqlite::Row<'_>| {
            Ok(DailyUsage {
                date: row.get(0)?,
                provider: row.get(1)?,
                sessions: row.get(2)?,
                prompts: row.get(3)?,
                assistant_turns: row.get(4)?,
                tool_calls: row.get(5)?,
                total_tokens: row.get(6)?,
            })
        };
        let days = if let Some(provider) = provider {
            daily_statement
                .query_map([provider], daily_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            daily_statement
                .query_map([], daily_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let busiest_day = days
            .iter()
            .cloned()
            .max_by_key(|day| day.prompts + day.tool_calls);

        let tool_sql = if provider.is_some() {
            &format!(
                r#"SELECT s.provider, tc.name, COUNT(*) calls
               FROM tool_calls tc JOIN sessions s ON s.id=tc.session_id
               WHERE {CATALOG_SESSION_FILTER} AND s.provider=?1 GROUP BY s.provider, tc.name ORDER BY calls DESC LIMIT 40"#
            )
        } else {
            &format!(
                r#"SELECT s.provider, tc.name, COUNT(*) calls
               FROM tool_calls tc JOIN sessions s ON s.id=tc.session_id
               WHERE {CATALOG_SESSION_FILTER} GROUP BY s.provider, tc.name ORDER BY calls DESC LIMIT 40"#
            )
        };
        let mut tool_statement = self.connection.prepare(tool_sql)?;
        let tool_mapper = |row: &rusqlite::Row<'_>| {
            Ok(ToolUsage {
                provider: row.get(0)?,
                name: row.get(1)?,
                calls: row.get(2)?,
            })
        };
        let top_tools = if let Some(provider) = provider {
            tool_statement
                .query_map([provider], tool_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            tool_statement
                .query_map([], tool_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let model_filter = if provider.is_some() {
            format!(" WHERE {CATALOG_SESSION_FILTER} AND s.provider=?1")
        } else {
            format!(" WHERE {CATALOG_SESSION_FILTER}")
        };
        let model_sql = format!(
            r#"SELECT s.provider, COALESCE(NULLIF(s.model,''), 'Bilinmiyor'), COUNT(*),
                      SUM((SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='user')),
                      SUM((SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id AND t.role='assistant')),
                      SUM((SELECT COUNT(*) FROM tool_calls tc WHERE tc.session_id=s.id)),
                      SUM(COALESCE((SELECT MAX(total_tokens) FROM usage_events u WHERE u.session_id=s.id),0))
               FROM sessions s{model_filter}
               GROUP BY s.provider, COALESCE(NULLIF(s.model,''), 'Bilinmiyor')
               ORDER BY COUNT(*) DESC"#
        );
        let mut model_statement = self.connection.prepare(&model_sql)?;
        let model_mapper = |row: &rusqlite::Row<'_>| {
            Ok(ModelUsage {
                provider: row.get(0)?,
                model: row.get(1)?,
                sessions: row.get(2)?,
                prompts: row.get(3)?,
                assistant_turns: row.get(4)?,
                tool_calls: row.get(5)?,
                total_tokens: row.get(6)?,
            })
        };
        let models = if let Some(provider) = provider {
            model_statement
                .query_map([provider], model_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            model_statement
                .query_map([], model_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(UsageAnalytics {
            total_sessions: providers.iter().map(|row| row.sessions).sum(),
            total_prompts: providers.iter().map(|row| row.prompts).sum(),
            total_assistant_turns: providers.iter().map(|row| row.assistant_turns).sum(),
            total_tool_calls: providers.iter().map(|row| row.tool_calls).sum(),
            total_tokens: providers.iter().map(|row| row.total_tokens).sum(),
            providers,
            days,
            top_tools,
            models,
            busiest_day,
        })
    }

    pub fn update_provider_titles(
        &mut self,
        provider: &str,
        titles: &[(String, String)],
    ) -> rusqlite::Result<usize> {
        let transaction = self.connection.transaction()?;
        let mut changed = 0;
        for (external_id, title) in titles {
            changed += transaction.execute(
                "UPDATE sessions SET title=?3 WHERE provider=?1 AND external_id=?2 AND title<>?3",
                params![provider, external_id, title],
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}

fn upsert_session_row(
    transaction: &Transaction<'_>,
    session_id: &str,
    parsed: &ParsedSession,
) -> rusqlite::Result<()> {
    transaction.execute(
        r#"
        INSERT INTO sessions(
          id, provider, source_kind, external_id, title, project_path, source_path,
          created_at, updated_at, model, archived, summary, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(provider, external_id) DO UPDATE SET
          source_kind=excluded.source_kind,
          title=CASE
            WHEN excluded.source_kind='claude_code_project' AND json_extract(sessions.metadata_json, '$.desktop_metadata')=1 THEN sessions.title
            WHEN excluded.title='' THEN sessions.title
            ELSE excluded.title
          END,
          project_path=COALESCE(excluded.project_path, sessions.project_path),
          source_path=excluded.source_path,
          created_at=COALESCE(sessions.created_at, excluded.created_at),
          updated_at=COALESCE(excluded.updated_at, sessions.updated_at),
          model=COALESCE(excluded.model, sessions.model),
          archived=MAX(sessions.archived, excluded.archived),
          summary=COALESCE(excluded.summary, sessions.summary),
          metadata_json=COALESCE(excluded.metadata_json, sessions.metadata_json),
          imported_at=CURRENT_TIMESTAMP
        "#,
        params![
            session_id,
            parsed.provider.as_str(),
            parsed.source_kind.as_str(),
            parsed.external_id,
            parsed.title.as_deref().unwrap_or("Untitled session"),
            parsed.project_path,
            parsed.source_path.to_string_lossy(),
            parsed.created_at,
            parsed.updated_at,
            parsed.model,
            parsed.archived as i64,
            parsed.summary,
            parsed.metadata_json,
        ],
    )?;
    Ok(())
}

fn enrich_session_row(
    transaction: &Transaction<'_>,
    session_id: &str,
    parsed: &ParsedSession,
) -> rusqlite::Result<()> {
    transaction.execute(
        r#"
        UPDATE sessions SET
          title=COALESCE(NULLIF(?2, ''), title),
          project_path=COALESCE(?3, project_path),
          created_at=COALESCE(created_at, ?4),
          updated_at=COALESCE(?5, updated_at),
          model=COALESCE(?6, model),
          archived=MAX(archived, ?7),
          summary=COALESCE(?8, summary),
          metadata_json=COALESCE(?9, metadata_json),
          imported_at=CURRENT_TIMESTAMP
        WHERE id=?1
        "#,
        params![
            session_id,
            parsed.title,
            parsed.project_path,
            parsed.created_at,
            parsed.updated_at,
            parsed.model,
            parsed.archived as i64,
            parsed.summary,
            parsed.metadata_json,
        ],
    )?;
    Ok(())
}

fn insert_turn(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn: &Turn,
) -> rusqlite::Result<()> {
    let turn_id = stable_id(&format!(
        "turn:{session_id}:{}:{}",
        turn.ordinal,
        turn.external_id.as_deref().unwrap_or_default()
    ));
    transaction.execute(
        r#"
        INSERT INTO turns(
          id, session_id, external_id, ordinal, role, created_at, text,
          event_type, model, parent_external_id, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
        params![
            turn_id,
            session_id,
            turn.external_id,
            turn.ordinal,
            turn.role.as_str(),
            turn.created_at,
            turn.text,
            turn.event_type,
            turn.model,
            turn.parent_external_id,
            turn.metadata_json,
        ],
    )?;
    for (ordinal, tool) in turn.tool_calls.iter().enumerate() {
        let tool_id = stable_id(&format!(
            "tool:{turn_id}:{}:{}",
            ordinal,
            tool.external_id.as_deref().unwrap_or_default()
        ));
        transaction.execute(
            r#"
            INSERT INTO tool_calls(
              id, session_id, turn_id, external_id, ordinal, name,
              arguments_json, result_text, status, duration_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                tool_id,
                session_id,
                turn_id,
                tool.external_id,
                ordinal as i64,
                tool.name,
                tool.arguments_json,
                tool.result_text,
                tool.status,
                tool.duration_ms,
            ],
        )?;
    }
    if let Some(usage) = &turn.usage {
        let usage_id = stable_id(&format!("usage:{turn_id}"));
        transaction.execute(
            r#"
            INSERT INTO usage_events(
              id, session_id, turn_id, input_tokens, output_tokens,
              cached_input_tokens, cache_write_input_tokens, reasoning_tokens, total_tokens, confidence, source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                usage_id,
                session_id,
                turn_id,
                usage.input_tokens,
                usage.output_tokens,
                usage.cached_input_tokens,
                usage.cache_write_input_tokens,
                usage.reasoning_tokens,
                usage.total_tokens,
                usage
                    .confidence
                    .map(|confidence| confidence.as_str())
                    .unwrap_or("estimated"),
                usage.source,
            ],
        )?;
    }
    Ok(())
}

fn stable_id(value: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, value.as_bytes()).to_string()
}

fn parse_role(value: &str) -> Role {
    match value {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "system" => Role::System,
        "tool" => Role::Tool,
        "reasoning" => Role::Reasoning,
        _ => Role::Unknown,
    }
}

fn fts_query(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(|part| format!("\"{}\"*", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    paths.extend(extract_windows_paths(text));
    let mut rest = text;
    while let Some(start) = rest.find("@\"") {
        let value = &rest[start + 2..];
        let Some(end) = value.find('"') else { break };
        if let Some(path) = normalize_file_path(&value[..end]) {
            paths.push(path);
        }
        rest = &value[end + 1..];
    }
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let value = &rest[start + 2..];
        let Some(end) = value.find(')') else { break };
        if let Some(path) = normalize_file_path(&value[..end]) {
            paths.push(path);
        }
        rest = &value[end + 1..];
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        collect_json_paths(&json, &mut paths);
    }
    paths
}

fn extract_explicit_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative) = text[cursor..].find('@') else {
            break;
        };
        let at = cursor + relative;
        let mut start = at + 1;
        let quoted = bytes.get(start) == Some(&b'"');
        if quoted {
            start += 1;
        }
        let tail = &text[start..];
        let end = if quoted {
            tail.find('"').unwrap_or(tail.len())
        } else {
            tail.find(['\r', '\n', '<', '>', '|']).unwrap_or(tail.len())
        };
        if let Some(path) = extract_windows_paths(tail[..end].trim()).into_iter().next() {
            paths.push(path);
        }
        cursor = start + end + usize::from(quoted && start + end < bytes.len());
    }

    for marker in [
        "# Files mentioned by the user:",
        "# Files mentioned by user:",
    ] {
        if let Some(start) = text.find(marker) {
            let attachment_block = &text[start + marker.len()..];
            let end = attachment_block
                .find("## My request:")
                .or_else(|| attachment_block.find("# My request:"))
                .or_else(|| attachment_block.find("Distinguish instructions"))
                .unwrap_or(attachment_block.len());
            paths.extend(extract_windows_paths(&attachment_block[..end]));
        }
    }
    paths
}

fn extract_windows_paths(text: &str) -> Vec<String> {
    const EXTENSIONS: &[&str] = &[
        "docx", "doc", "pdf", "md", "txt", "rtf", "xlsx", "xls", "csv", "pptx", "ppt", "json",
        "jsonl", "toml", "yaml", "yml", "xml", "html", "css", "tsx", "ts", "jsx", "js", "py", "rs",
        "png", "jpg", "jpeg", "gif", "webp", "svg", "zip", "7z",
    ];
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut cursor = 0;
    while cursor + 2 < bytes.len() {
        if !bytes[cursor].is_ascii_alphabetic()
            || bytes[cursor + 1] != b':'
            || !matches!(bytes[cursor + 2], b'\\' | b'/')
        {
            cursor += 1;
            continue;
        }
        let start = cursor;
        if start > 0 && bytes[start - 1] == b'"' {
            cursor += 3;
            continue;
        }
        let mut end = start + 3;
        while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n' | b'"' | b'<' | b'>' | b'|')
        {
            end += 1;
        }
        let candidate = &text[start..end];
        let lower = candidate.to_ascii_lowercase();
        let mut extension_end = None;
        for extension in EXTENSIONS {
            let needle = format!(".{extension}");
            let mut search_from = 0;
            while let Some(index) = lower[search_from..].find(&needle) {
                let candidate_end = search_from + index + needle.len();
                let boundary = lower
                    .as_bytes()
                    .get(candidate_end)
                    .is_none_or(|value| !value.is_ascii_alphanumeric());
                if boundary && extension_end.is_none_or(|current| candidate_end < current) {
                    extension_end = Some(candidate_end);
                }
                search_from = candidate_end;
            }
        }
        if let Some(relative_end) = extension_end {
            end = start + relative_end;
        } else if let Some(relative_end) = candidate.find("  ") {
            end = start + relative_end;
        } else {
            let leaf_has_space = candidate
                .rsplit(['\\', '/'])
                .next()
                .is_some_and(|leaf| leaf.contains(char::is_whitespace));
            if leaf_has_space && !std::path::Path::new(candidate.trim()).exists() {
                cursor = end;
                continue;
            }
        }
        let path = text[start..end]
            .trim_end()
            .trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if let Some(path) = normalize_file_path(path) {
            found.push(path);
        }
        cursor = end.max(start + 3);
    }
    let mut seen = std::collections::HashSet::new();
    found.retain(|path| seen.insert(path.to_ascii_lowercase()));
    found
}

fn collect_json_paths(value: &serde_json::Value, paths: &mut Vec<String>) {
    match value {
        serde_json::Value::String(_) => {}
        serde_json::Value::Array(values) => {
            for value in values {
                collect_json_paths(value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "path" | "filePath" | "filename") {
                    if let Some(value) = value.as_str().and_then(normalize_file_path) {
                        paths.push(value);
                    }
                } else if value.is_array() || value.is_object() {
                    collect_json_paths(value, paths);
                }
            }
        }
        _ => {}
    }
}

fn normalize_file_path(value: &str) -> Option<String> {
    let mut value = value.trim().trim_matches(['"', '\'', '<', '>']);
    value = value
        .strip_prefix("file:///")
        .or_else(|| value.strip_prefix("file://"))
        .unwrap_or(value);
    if value.starts_with('/')
        && value.as_bytes().get(2) == Some(&b':')
        && matches!(value.as_bytes().get(3), Some(b'\\' | b'/'))
    {
        value = &value[1..];
    }
    let looks_windows = value.len() > 3
        && value.as_bytes().get(1) == Some(&b':')
        && matches!(value.as_bytes().get(2), Some(b'\\' | b'/'));
    let looks_unix = value.starts_with('/') && !value.starts_with("//");
    (looks_windows || looks_unix).then(|| {
        let normalized = value.replace("\\\\", "\\");
        if looks_windows {
            normalized.replace('/', "\\")
        } else {
            normalized
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Provider, SourceKind};
    use std::path::PathBuf;

    fn session() -> ParsedSession {
        ParsedSession {
            provider: Provider::Codex,
            source_kind: SourceKind::CodexRollout,
            external_id: "session-1".to_string(),
            title: Some("Find the parser".to_string()),
            project_path: Some("/repo".to_string()),
            source_path: PathBuf::from("/tmp/session.jsonl"),
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            updated_at: Some("2026-01-01T00:01:00Z".to_string()),
            model: Some("gpt-test".to_string()),
            archived: false,
            summary: None,
            turns: vec![Turn {
                external_id: Some("turn-1".to_string()),
                ordinal: 0,
                prompt_ordinal: None,
                role: Role::User,
                created_at: None,
                text: "Find the parser".to_string(),
                event_type: Some("message".to_string()),
                model: None,
                parent_external_id: None,
                usage: None,
                tool_calls: Vec::new(),
                metadata_json: None,
            }],
            metadata_json: None,
        }
    }

    #[test]
    fn imports_and_searches_a_session_idempotently() {
        let mut archive = Archive::in_memory().unwrap();
        let mut parsed = session();
        parsed.turns[0].tool_calls.push(ToolCall {
            external_id: Some("tool-1".into()),
            name: "needle_tool".into(),
            arguments_json: Some("{\"path\":\"archive\"}".into()),
            result_text: Some("indexed tool result".into()),
            status: Some("complete".into()),
            duration_ms: Some(12),
        });
        archive
            .import_session(&parsed, "hash-1", 10, Some(1))
            .unwrap();
        archive
            .import_session(&parsed, "hash-2", 11, Some(2))
            .unwrap();
        let sessions = archive.list_sessions(None, Some("parser"), 50).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].turn_count, 1);
        assert_eq!(
            archive
                .list_sessions(None, Some("needle_tool"), 50)
                .unwrap()
                .len(),
            1
        );
        let page = archive
            .load_turn_page(&sessions[0].id, "prompts", 0, 20, None)
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.turns[0].prompt_ordinal, Some(1));
        assert_eq!(page.turns[0].tool_calls.len(), 1);
        let usage = archive.usage_analytics(None).unwrap();
        assert_eq!(usage.total_sessions, 1);
        assert_eq!(usage.total_prompts, 1);
        assert_eq!(usage.total_tool_calls, 1);
        assert_eq!(usage.top_tools[0].name, "needle_tool");
        assert_eq!(usage.models[0].model, "gpt-test");
        let exact = archive
            .load_turn_page(&sessions[0].id, "all", 0, 20, Some("needle_tool"))
            .unwrap();
        assert_eq!(exact.total, 1);
        let absent = archive
            .load_turn_page(&sessions[0].id, "all", 0, 20, Some("not present"))
            .unwrap();
        assert_eq!(absent.total, 0);
        archive
            .update_provider_titles("codex", &[("session-1".into(), "Exact task name".into())])
            .unwrap();
        assert_eq!(
            archive.list_sessions(None, None, 10).unwrap()[0].title,
            "Exact task name"
        );
    }

    #[test]
    fn conversation_pages_count_messages_not_tool_only_envelopes() {
        let mut archive = Archive::in_memory().unwrap();
        let mut parsed = session();
        parsed.turns.push(Turn {
            external_id: Some("tool-envelope".into()),
            ordinal: 1,
            prompt_ordinal: None,
            role: Role::Assistant,
            created_at: None,
            text: String::new(),
            event_type: Some("assistant".into()),
            model: None,
            parent_external_id: None,
            usage: None,
            tool_calls: vec![ToolCall {
                external_id: Some("call-1".into()),
                name: "run_command".into(),
                arguments_json: Some("{}".into()),
                result_text: None,
                status: Some("completed".into()),
                duration_ms: None,
            }],
            metadata_json: None,
        });
        archive
            .import_session(&parsed, "messages", 10, Some(1))
            .unwrap();
        let id = archive.list_sessions(None, None, 10).unwrap()[0].id.clone();
        let conversation = archive
            .load_turn_page(&id, "conversation", 0, 20, None)
            .unwrap();
        assert_eq!(conversation.total, 1);
        assert_eq!(conversation.turns[0].role, Role::User);
        assert_eq!(
            archive
                .load_turn_page(&id, "tools", 0, 20, None)
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn claude_metadata_title_and_archive_survive_transcript_import() {
        let mut archive = Archive::in_memory().unwrap();
        let mut metadata = session();
        metadata.provider = Provider::Claude;
        metadata.source_kind = SourceKind::ClaudeDesktopMetadata;
        metadata.title = Some("Claude'daki gerçek başlık".into());
        metadata.archived = true;
        metadata.turns.clear();
        metadata.metadata_json = Some(r#"{"desktop_metadata":true}"#.into());
        let mut transcript = session();
        transcript.provider = Provider::Claude;
        transcript.source_kind = SourceKind::ClaudeCodeProject;
        transcript.title = Some("İlk prompttan türetilen başlık".into());
        archive
            .import_session(&metadata, "meta", 10, Some(1))
            .unwrap();
        archive
            .import_session(&transcript, "body", 20, Some(2))
            .unwrap();
        let row = archive
            .list_sessions(Some("claude"), None, 10)
            .unwrap()
            .remove(0);
        assert_eq!(row.title, "Claude'daki gerçek başlık");
        assert!(row.archived);
        assert_eq!(row.turn_count, 1);
    }

    #[test]
    fn metadata_only_sessions_stay_out_of_the_catalog() {
        let mut archive = Archive::in_memory().unwrap();
        let mut metadata = session();
        metadata.turns.clear();
        metadata.archived = true;
        archive
            .import_session(&metadata, "metadata-only", 10, Some(1))
            .unwrap();
        assert!(archive.list_sessions(None, None, 10).unwrap().is_empty());
    }

    #[test]
    fn codex_worker_streams_without_a_user_prompt_stay_out_of_the_catalog() {
        let mut archive = Archive::in_memory().unwrap();
        let mut worker = session();
        worker.title = None;
        worker.turns[0].role = Role::Assistant;
        worker.turns[0].text = "background worker output".into();
        worker.turns[0].tool_calls.push(ToolCall {
            external_id: None,
            name: "read_file".into(),
            arguments_json: None,
            result_text: None,
            status: Some("completed".into()),
            duration_ms: None,
        });
        archive
            .import_session(&worker, "worker", 10, Some(1))
            .unwrap();
        assert!(archive
            .list_sessions(Some("codex"), None, 10)
            .unwrap()
            .is_empty());
        assert!(archive.provider_counts().unwrap().is_empty());
    }

    #[test]
    fn provider_subagents_and_inherited_fork_snapshots_stay_out_of_the_catalog() {
        let mut archive = Archive::in_memory().unwrap();
        let mut subagent = session();
        subagent.provider = Provider::Grok;
        subagent.external_id = "subagent".into();
        subagent.metadata_json = Some(r#"{"session_kind":"subagent"}"#.into());
        archive
            .import_session(&subagent, "subagent", 10, Some(1))
            .unwrap();

        let mut inherited_fork = session();
        inherited_fork.external_id = "fork".into();
        inherited_fork.metadata_json = Some(r#"{"inherited_fork_snapshot":true}"#.into());
        archive
            .import_session(&inherited_fork, "fork", 10, Some(1))
            .unwrap();

        assert!(archive.list_sessions(None, None, 10).unwrap().is_empty());
        assert!(archive.provider_counts().unwrap().is_empty());
        assert_eq!(archive.usage_analytics(None).unwrap().total_sessions, 0);
    }

    #[test]
    fn file_references_keep_user_and_model_provenance() {
        let mut archive = Archive::in_memory().unwrap();
        let mut parsed = session();
        parsed.turns[0].text = r#"Use E:\Obsidian Vaults\Trace Analysis\system\index.md"#.into();
        parsed.turns.push(Turn {
            external_id: Some("assistant-path".into()),
            ordinal: 1,
            prompt_ordinal: None,
            role: Role::Assistant,
            created_at: None,
            text: r#"Saved [index](</E:/Obsidian Vaults/Trace Analysis/system/index.md>)"#.into(),
            event_type: Some("message".into()),
            model: None,
            parent_external_id: None,
            usage: None,
            tool_calls: vec![ToolCall {
                external_id: None,
                name: "read_file".into(),
                arguments_json: Some(r#"{"path":"E:\\trace analysis\\workflow.md"}"#.into()),
                result_text: None,
                status: Some("completed".into()),
                duration_ms: None,
            }],
            metadata_json: None,
        });
        let id = archive
            .import_session(&parsed, "origins", 10, Some(1))
            .unwrap();
        let references = archive.session_file_references(&id).unwrap();
        assert_eq!(
            references[0].path,
            r"E:\Obsidian Vaults\Trace Analysis\system\index.md"
        );
        assert_eq!(references[0].origins, vec!["user", "assistant"]);
        assert_eq!(references[1].origins, vec!["tool"]);
    }

    #[test]
    fn provenance_paths_are_not_reported_as_chat_file_mentions() {
        let mut archive = Archive::in_memory().unwrap();
        let parsed = session();
        let id = archive
            .import_session(&parsed, "paths", 10, Some(1))
            .unwrap();
        assert!(archive.session_file_references(&id).unwrap().is_empty());
    }

    #[test]
    fn file_path_extraction_ignores_arbitrary_json_strings() {
        let paths = extract_file_paths(
            r#"{"path":"E:\\trace analysis\\workflow.md","prompt":"C:\\Users\\ismai\\Documents is not a file reference"}"#,
        );
        assert_eq!(paths, vec![r"E:\trace analysis\workflow.md"]);
    }

    #[test]
    fn file_path_extraction_handles_unquoted_mentions_and_directories() {
        let paths = extract_file_paths(
            "@C:\\Users\\ismai\\Downloads\\JRFID\\JRFID_Article.docx and @C:\\Users\\ismai\\Downloads\\JRFID\\High Gain Suspended Patch Antenna Element with Contactless Capacitive Coupling Feed.docx\nE:\\trace analysis\\TRACE_ANALYSIS_MASTER_WORKFLOW    burada\nE:/Obsidian Vaults/Trace Analysis/system/index.md",
        );
        assert_eq!(
            paths,
            vec![
                r"C:\Users\ismai\Downloads\JRFID\JRFID_Article.docx",
                r"C:\Users\ismai\Downloads\JRFID\High Gain Suspended Patch Antenna Element with Contactless Capacitive Coupling Feed.docx",
                r"E:\trace analysis\TRACE_ANALYSIS_MASTER_WORKFLOW",
                r"E:\Obsidian Vaults\Trace Analysis\system\index.md",
            ]
        );
    }

    #[test]
    fn export_paths_only_accept_explicit_mentions_and_attachment_block() {
        let paths = extract_explicit_file_paths(
            "Bare E:\\project\\everything should not export.md\n@E:\\picked\\note.md\n# Files mentioned by the user:\n## C:\\Temp\\dragged.pdf\n## My request:\nIgnore E:\\project\\also-not-exported.md",
        );
        assert_eq!(paths, vec![r"E:\picked\note.md", r"C:\Temp\dragged.pdf"]);
    }
}
