use crate::Document;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

struct Pending {
    session: String,
    revision: String,
    expected: u64,
    count: u64,
    last_seq: Option<u64>,
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
        db.execute_batch("PRAGMA busy_timeout=1000; PRAGMA journal_mode=WAL; PRAGMA cache_size=-8192; PRAGMA mmap_size=0; PRAGMA temp_store=FILE; PRAGMA max_page_count=65536;
            CREATE TABLE IF NOT EXISTS sessions (session TEXT PRIMARY KEY, revision TEXT NOT NULL, documents INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS documents (session TEXT NOT NULL, seq INTEGER NOT NULL, text TEXT NOT NULL, folded TEXT NOT NULL, PRIMARY KEY(session,seq));
            CREATE TEMP TABLE staging (seq INTEGER PRIMARY KEY, text TEXT NOT NULL, folded TEXT NOT NULL);
            PRAGMA temp.cache_size=-2048; PRAGMA user_version=1;").map_err(sql)?;
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
        let bytes: usize = documents.iter().map(|d| d.text.len()).sum();
        // Account conservatively for Unicode lowercase expansion and staging.
        if pending.bytes + bytes > 32 * 1024 * 1024 {
            return Err("SESSION_INDEX_TEXT_BUDGET_EXCEEDED".into());
        }
        for doc in &documents {
            if doc.seq > 9_007_199_254_740_991 || last.is_some_and(|seq| doc.seq <= seq) {
                return Err("DOCUMENT_SEQUENCE_INVALID".into());
            }
            if pending.from_seq.is_some_and(|seq| doc.seq < seq) {
                return Err("DOCUMENT_BEFORE_INCREMENTAL_BASE".into());
            }
            if doc.text.len() > 1_048_576 {
                return Err("DOCUMENT_TOO_LARGE".into());
            }
            last = Some(doc.seq);
        }
        let tx = self.db.transaction().map_err(sql)?;
        {
            let mut insert = tx
                .prepare_cached("INSERT INTO staging VALUES (?1,?2,?3)")
                .map_err(sql)?;
            for doc in &documents {
                insert
                    .execute(params![doc.seq, doc.text, doc.text.to_lowercase()])
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
        tx.execute(
            "DELETE FROM documents WHERE session=?1 AND (?2 IS NULL OR seq>=?2)",
            params![session, p.from_seq],
        )
        .map_err(sql)?;
        tx.execute(
            "INSERT INTO documents SELECT ?1,seq,text,folded FROM staging",
            [session],
        )
        .map_err(sql)?;
        let count: u64 = tx
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE session=?1",
                [session],
                |r| r.get(0),
            )
            .map_err(sql)?;
        tx.execute("INSERT INTO sessions VALUES (?1,?2,?3) ON CONFLICT(session) DO UPDATE SET revision=excluded.revision,documents=excluded.documents",params![session,p.revision,count]).map_err(sql)?;
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
    ) -> Result<Value, String> {
        if query.is_empty() || query.len() > 1024 || limit == 0 || limit > 100 {
            return Err("INVALID_SEARCH".into());
        }
        if let Some(s) = session {
            identity(s)?;
        }
        // Literal Unicode-lowercase substring contract, not a replacement for
        // official ranked FTS syntax. No wildcards or SQL fragments are accepted.
        let needle = query.to_lowercase();
        let mut stmt=self.db.prepare("SELECT d.session,d.seq,d.text,s.revision FROM documents d JOIN sessions s USING(session)
            WHERE (?1 IS NULL OR d.session=?1) AND instr(d.folded,?2)>0 ORDER BY d.session,d.seq LIMIT ?3").map_err(sql)?;
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
        Ok(json!({"hits":hits,"has_more":has_more}))
    }
}
