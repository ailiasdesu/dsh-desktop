use crate::Document;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

// Derived data only. Bounds apply independently of upstream history size. A miss
// or explicit quota error sends the caller back to the authoritative source.
const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_CACHE_SESSIONS: u64 = 4096;
const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;
const ROW_OVERHEAD: usize = 128;

fn policy(db: &Connection) -> Result<(u64, u64), String> {
    let (bytes, sessions): (u64, u64) = db
        .query_row(
            "SELECT max_bytes,max_sessions FROM cache_policy WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    if bytes == 0 || bytes > MAX_CACHE_BYTES || sessions == 0 || sessions > MAX_CACHE_SESSIONS {
        return Err("INVALID_INDEX_CACHE_POLICY".into());
    }
    Ok((bytes, sessions))
}

fn maintain_wal(db: &Connection) -> Result<(), String> {
    // A reader in another helper can pin old WAL frames indefinitely. Stop
    // writes if truncation is blocked once the small threshold is reached;
    // journal_size_limit alone does not bound a reader-pinned WAL.
    if let Some(path) = db.path() {
        if std::fs::metadata(format!("{path}-wal"))
            .map(|m| m.len() > 4 * 1024 * 1024)
            .unwrap_or(false)
        {
            let busy: u32 = db
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
                .map_err(sql)?;
            if busy != 0 {
                return Err("INDEX_CACHE_CHECKPOINT_BUSY".into());
            }
        }
    }
    Ok(())
}

fn touch(db: &Connection, session: &str) -> Result<(), String> {
    db.execute("UPDATE sessions SET last_used=(SELECT COALESCE(MAX(last_used),0)+1 FROM sessions) WHERE session=?1", [session]).map_err(sql)?;
    Ok(())
}

struct Pending {
    session: String,
    revision: String,
    expected: u64,
    count: u64,
    last_seq: Option<(u64, u32)>,
    from_seq: Option<u64>,
    original_revision: Option<String>,
    bytes: usize,
}

pub struct TextIndex {
    db: Connection,
    pending: Option<Pending>,
}

fn sql(error: rusqlite::Error) -> String {
    error.to_string()
}
fn identity(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 1024 || value.contains('\0') {
        Err("INVALID_IDENTITY".into())
    } else {
        Ok(())
    }
}

impl TextIndex {
    pub fn open(cache: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
        // Own database only. Never receives the path of an official index DB.
        let db = Connection::open(cache.join("desktop-text-v1.sqlite")).map_err(sql)?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql)?;
        if version != 0 && version != 1 {
            return Err("UNSUPPORTED_INDEX_SCHEMA".into());
        }
        db.execute_batch("PRAGMA busy_timeout=1000; PRAGMA journal_mode=WAL; PRAGMA cache_size=-8192; PRAGMA mmap_size=0; PRAGMA temp_store=FILE; PRAGMA wal_autocheckpoint=256; PRAGMA journal_size_limit=4194304;
            CREATE TABLE IF NOT EXISTS sessions (session TEXT PRIMARY KEY, revision TEXT NOT NULL, documents INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS documents (session TEXT NOT NULL, seq INTEGER NOT NULL, text TEXT NOT NULL, folded TEXT NOT NULL, PRIMARY KEY(session,seq));
            CREATE TEMP TABLE staging (seq INTEGER NOT NULL, part INTEGER NOT NULL, text TEXT NOT NULL, folded TEXT NOT NULL, preview TEXT, PRIMARY KEY(seq,part));
            PRAGMA temp.cache_size=-2048; PRAGMA user_version=1;").map_err(sql)?;
        // Physical caps are separate from logical payload accounting: SQLite
        // pages include B-tree/index overhead. WAL can hold at most one capped
        // database transaction; staging is on disk and capped independently.
        for schema in ["main", "temp"] {
            let page_size: u64 = db
                .pragma_query_value(
                    Some(rusqlite::DatabaseName::Attached(schema)),
                    "page_size",
                    |r| r.get(0),
                )
                .map_err(sql)?;
            db.pragma_update(
                Some(rusqlite::DatabaseName::Attached(schema)),
                "max_page_count",
                2 * MAX_CACHE_BYTES / page_size,
            )
            .map_err(sql)?;
        }
        maintain_wal(&db)?;
        // Additive v1 migration, serialized with other helper connections.
        db.execute_batch("BEGIN IMMEDIATE").map_err(sql)?;
        let migration = (|| -> Result<(), String> {
            let doc_columns = {
                let mut stmt = db.prepare("PRAGMA table_info(documents)").map_err(sql)?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(1)).map_err(sql)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(sql)?
            };
            if !doc_columns.iter().any(|s| s == "part") {
                db.execute_batch("ALTER TABLE documents RENAME TO documents_legacy;
                    CREATE TABLE documents(session TEXT NOT NULL,seq INTEGER NOT NULL,part INTEGER NOT NULL,text TEXT NOT NULL,folded TEXT NOT NULL,preview TEXT,PRIMARY KEY(session,seq,part));
                    INSERT INTO documents(session,seq,part,text,folded,preview) SELECT session,seq,0,text,folded,NULL FROM documents_legacy;
                    DROP TABLE documents_legacy;").map_err(sql)?;
            }
            let mut stmt = db.prepare("PRAGMA table_info(sessions)").map_err(sql)?;
            let columns = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            if !columns.iter().any(|s| s == "bytes") {
                db.execute_batch("ALTER TABLE sessions ADD COLUMN bytes INTEGER NOT NULL DEFAULT 0;
                    ALTER TABLE sessions ADD COLUMN last_used INTEGER NOT NULL DEFAULT 0;
                    UPDATE sessions SET bytes=COALESCE((SELECT SUM(length(CAST(text AS BLOB))+length(CAST(folded AS BLOB))+128) FROM documents WHERE documents.session=sessions.session),0);").map_err(sql)?;
            }
            db.execute_batch("CREATE TABLE IF NOT EXISTS cache_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1), max_bytes INTEGER NOT NULL, max_sessions INTEGER NOT NULL);
                CREATE INDEX IF NOT EXISTS sessions_lru ON sessions(last_used,session);").map_err(sql)?;
            db.execute(
                "INSERT OR IGNORE INTO cache_policy VALUES(1,?1,?2)",
                params![MAX_CACHE_BYTES, MAX_CACHE_SESSIONS],
            )
            .map_err(sql)?;
            policy(&db)?;
            Ok(())
        })();
        if let Err(error) = migration {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error);
        }
        db.execute_batch("COMMIT").map_err(sql)?;
        Ok(Self { db, pending: None })
    }

    pub fn begin(
        &mut self,
        session: &str,
        revision: &str,
        expected: u64,
        base_revision: Option<&str>,
        from_seq: Option<u64>,
    ) -> Result<Value, String> {
        identity(session)?;
        identity(revision)?;
        if expected > 10_000_000 {
            return Err("INDEX_TOO_LARGE".into());
        }
        if self.pending.is_some() {
            return Err("INDEX_IMPORT_BUSY".into());
        }
        if base_revision.is_some() != from_seq.is_some() {
            return Err("INCOMPLETE_INCREMENTAL_BASE".into());
        }
        let original_revision = self.state(session)?["revision"].as_str().map(str::to_owned);
        if base_revision.is_some() && original_revision.as_deref() != base_revision {
            return Err("INDEX_REVISION_MISMATCH".into());
        }
        if from_seq.is_some_and(|seq| seq > 9_007_199_254_740_991) {
            return Err("DOCUMENT_SEQUENCE_INVALID".into());
        }
        self.db.execute("DELETE FROM staging", []).map_err(sql)?;
        self.pending = Some(Pending {
            session: session.into(),
            revision: revision.into(),
            expected,
            count: 0,
            last_seq: None,
            from_seq,
            original_revision,
            bytes: 0,
        });
        Ok(json!({"started":true}))
    }

    pub fn append(&mut self, session: &str, documents: Vec<Document>) -> Result<Value, String> {
        let pending = self.pending.as_ref().ok_or("NO_INDEX_IMPORT")?;
        if pending.session != session {
            return Err("WRONG_INDEX_IMPORT".into());
        }
        if documents.len() > 2048 || pending.count + documents.len() as u64 > pending.expected {
            return Err("DOCUMENT_COUNT_EXCEEDED".into());
        }
        let mut last = pending.last_seq;
        let input_bytes: usize = documents
            .iter()
            .map(|d| d.text.len() + d.preview.as_ref().map_or(0, String::len))
            .sum();
        if input_bytes > MAX_BATCH_BYTES {
            return Err("INDEX_BATCH_TOO_LARGE".into());
        }
        // Only this bounded batch is resident. Charge actual UTF-8 lowercase
        // expansion plus conservative row overhead, including empty documents.
        let bytes: usize = documents
            .iter()
            .map(|d| {
                d.text.len()
                    + d.text.to_lowercase().len()
                    + d.preview.as_ref().map_or(0, String::len)
                    + ROW_OVERHEAD
            })
            .sum();
        if pending.bytes as u64 + bytes as u64 > policy(&self.db)?.0 {
            return Err("SESSION_INDEX_TEXT_BUDGET_EXCEEDED".into());
        }
        for doc in &documents {
            if doc.seq > 9_007_199_254_740_991 || last.is_some_and(|(seq, _)| doc.seq < seq) {
                return Err("DOCUMENT_SEQUENCE_INVALID".into());
            }
            let required_part = match last {
                Some((seq, part)) if seq == doc.seq => {
                    part.checked_add(1).ok_or("DOCUMENT_PART_INVALID")?
                }
                _ => 0,
            };
            if doc.part != required_part {
                return Err("DOCUMENT_PART_INVALID".into());
            }
            if doc
                .preview
                .as_ref()
                .is_some_and(|p| p.chars().take(241).count() > 240)
            {
                return Err("DOCUMENT_PREVIEW_TOO_LARGE".into());
            }
            if pending.from_seq.is_some_and(|seq| doc.seq < seq) {
                return Err("DOCUMENT_BEFORE_INCREMENTAL_BASE".into());
            }
            if doc.text.len() > 1_048_576 {
                return Err("DOCUMENT_TOO_LARGE".into());
            }
            last = Some((doc.seq, doc.part));
        }
        let tx = self.db.transaction().map_err(sql)?;
        {
            let mut insert = tx
                .prepare_cached("INSERT INTO staging VALUES (?1,?2,?3,?4,?5)")
                .map_err(sql)?;
            for doc in &documents {
                insert
                    .execute(params![
                        doc.seq,
                        doc.part,
                        doc.text,
                        doc.text.to_lowercase(),
                        doc.preview
                    ])
                    .map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)?;
        let pending = self.pending.as_mut().unwrap();
        pending.count += documents.len() as u64;
        pending.last_seq = last;
        pending.bytes += bytes;
        Ok(json!({"staged":pending.count}))
    }

    pub fn commit(&mut self, session: &str) -> Result<Value, String> {
        let p = self.pending.as_ref().ok_or("NO_INDEX_IMPORT")?;
        if p.session != session {
            return Err("WRONG_INDEX_IMPORT".into());
        }
        if p.count != p.expected {
            return Err("DOCUMENT_COUNT_MISMATCH".into());
        }
        maintain_wal(&self.db)?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let current: Option<String> = tx
            .query_row(
                "SELECT revision FROM sessions WHERE session=?1",
                [session],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        if current != p.original_revision {
            return Err("INDEX_REVISION_CHANGED_DURING_IMPORT".into());
        }
        let (max_bytes, max_sessions) = policy(&tx)?;
        let retained: u64 = tx.query_row(
            "SELECT COALESCE(SUM(length(CAST(text AS BLOB))+length(CAST(folded AS BLOB))+COALESCE(length(CAST(preview AS BLOB)),0)+128),0) FROM documents WHERE session=?1 AND ?2 IS NOT NULL AND seq<?2",
            params![session,p.from_seq], |r| r.get(0),
        ).map_err(sql)?;
        let new_bytes = retained + p.bytes as u64;
        if new_bytes > max_bytes {
            return Err("SESSION_INDEX_TEXT_BUDGET_EXCEEDED".into());
        }
        // Evictions and replacement share the revision-checked transaction:
        // quota, disk-full, or concurrent replacement errors preserve all old
        // data. Never claim that a partial index is a successful import.
        loop {
            let (other_bytes, other_count): (u64, u64) = tx
                .query_row(
                    "SELECT COALESCE(SUM(bytes),0),COUNT(*) FROM sessions WHERE session<>?1",
                    [session],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(sql)?;
            if other_bytes + new_bytes <= max_bytes && other_count < max_sessions {
                break;
            }
            let victim: String = tx.query_row("SELECT session FROM sessions WHERE session<>?1 ORDER BY last_used,session LIMIT 1", [session], |r| r.get(0)).map_err(sql)?;
            tx.execute("DELETE FROM documents WHERE session=?1", [&victim])
                .map_err(sql)?;
            tx.execute("DELETE FROM sessions WHERE session=?1", [&victim])
                .map_err(sql)?;
        }
        tx.execute(
            "DELETE FROM documents WHERE session=?1 AND (?2 IS NULL OR seq>=?2)",
            params![session, p.from_seq],
        )
        .map_err(sql)?;
        tx.execute(
            "INSERT INTO documents SELECT ?1,seq,part,text,folded,preview FROM staging",
            [session],
        )
        .map_err(sql)?;
        let count: u64 = tx
            .query_row(
                "SELECT COUNT(DISTINCT seq) FROM documents WHERE session=?1",
                [session],
                |r| r.get(0),
            )
            .map_err(sql)?;
        tx.execute("INSERT INTO sessions(session,revision,documents,bytes,last_used) VALUES (?1,?2,?3,?4,0) ON CONFLICT(session) DO UPDATE SET revision=excluded.revision,documents=excluded.documents,bytes=excluded.bytes",params![session,p.revision,count,new_bytes]).map_err(sql)?;
        touch(&tx, session)?;
        tx.commit().map_err(sql)?;
        self.pending = None;
        self.db.execute("DELETE FROM staging", []).map_err(sql)?;
        self.state(session)
    }

    pub fn abort(&mut self) -> Result<Value, String> {
        self.db.execute("DELETE FROM staging", []).map_err(sql)?;
        self.pending = None;
        Ok(json!({"aborted":true}))
    }

    pub fn state(&self, session: &str) -> Result<Value, String> {
        identity(session)?;
        let row = self
            .db
            .query_row(
                "SELECT revision,documents FROM sessions WHERE session=?1",
                [session],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(sql)?;
        if row.is_some() && maintain_wal(&self.db).is_ok() {
            // LRU is advisory; a pinned reader must not turn a valid hit into
            // a failure or make a completed commit look unsuccessful.
            let _ = touch(&self.db, session);
        }
        Ok(match row {
            Some((revision, documents)) => json!({"revision":revision,"documents":documents}),
            None => Value::Null,
        })
    }

    pub fn delete(&mut self, session: &str) -> Result<Value, String> {
        identity(session)?;
        if self.pending.as_ref().is_some_and(|p| p.session == session) {
            return Err("INDEX_IMPORT_BUSY".into());
        }
        maintain_wal(&self.db)?;
        let tx = self.db.transaction().map_err(sql)?;
        tx.execute("DELETE FROM documents WHERE session=?1", [session])
            .map_err(sql)?;
        tx.execute("DELETE FROM sessions WHERE session=?1", [session])
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(json!({"deleted":true}))
    }

    pub fn search(
        &self,
        query: &str,
        session: Option<&str>,
        limit: usize,
        expected_revision: Option<&str>,
    ) -> Result<Value, String> {
        if query.is_empty() || query.len() > 1024 || limit == 0 || limit > 100 {
            return Err("INVALID_SEARCH".into());
        }
        if let Some(s) = session {
            identity(s)?;
        }
        let snapshot=self.db.unchecked_transaction().map_err(sql)?;
        if let Some(expected)=expected_revision {
            identity(expected)?;
            let scope=session.ok_or("SCOPED_REVISION_REQUIRED")?;
            let actual:Option<String>=snapshot.query_row("SELECT revision FROM sessions WHERE session=?1",[scope],|r|r.get(0)).optional().map_err(sql)?;
            if actual.as_deref()!=Some(expected){return Err("INDEX_REVISION_MISMATCH".into());}
        }
        // Literal Unicode-lowercase substring contract, not a replacement for
        // official ranked FTS syntax. No wildcards or SQL fragments are accepted.
        let needle = query.to_lowercase();
        let scope_predicate=if session.is_some(){"d.session=?1"}else{"?1 IS NULL"};
        let search_sql=format!("SELECT d.session,d.seq,COALESCE(first.preview,first.text),s.revision FROM documents d JOIN sessions s USING(session)
            JOIN documents first ON first.session=d.session AND first.seq=d.seq AND first.part=0
            WHERE {scope_predicate} AND instr(d.folded,?2)>0 GROUP BY d.session,d.seq ORDER BY d.session,d.seq LIMIT ?3");
        let mut stmt=snapshot.prepare(&search_sql).map_err(sql)?;
        let rows = stmt
            .query_map(params![session, needle, limit + 1], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(sql)?;
        let mut hits = Vec::new();
        let mut has_more = false;
        for row in rows {
            let (session, seq, text, revision) = row.map_err(sql)?;
            if hits.len() == limit {
                has_more = true;
                break;
            }
            // Return a bounded preview. Full content stays on disk.
            let preview: String = text.chars().take(240).collect();
            hits.push(json!({"session":session,"seq":seq,"preview":preview,"revision":revision}));
        }
        drop(stmt);
        snapshot.commit().map_err(sql)?;
        // A scoped no-match search still counts as access. Global searches
        // touch only returned sessions, avoiding writes proportional to history.
        if maintain_wal(&self.db).is_ok() {
            if let Some(session) = session {
                let _ = touch(&self.db, session);
            } else {
                let mut touched = std::collections::HashSet::new();
                for hit in &hits {
                    if let Some(session) = hit["session"].as_str() {
                        if touched.insert(session) {
                            let _ = touch(&self.db, session);
                        }
                    }
                }
            }
        }
        Ok(json!({"hits":hits,"has_more":has_more}))
    }
}
