use dsh_native_helper::{Helper, Request};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "dsh-cache-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn helper(&self) -> Helper {
        Helper::open(&self.0).unwrap()
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join("desktop-text-v1.sqlite")).unwrap()
    }
    fn policy(&self, bytes: u64, sessions: u64) {
        self.db()
            .execute(
                "UPDATE cache_policy SET max_bytes=?1,max_sessions=?2",
                params![bytes, sessions],
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = std::env::temp_dir().canonicalize().unwrap();
        if self.0.parent() == Some(root.as_path())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dsh-cache-limits-")
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn call(h: &mut Helper, mut v: Value) -> Value {
    v["id"] = json!(1);
    v["version"] = json!(1);
    h.execute(serde_json::from_value::<Request>(v).unwrap())
}
fn ok(h: &mut Helper, v: Value) -> Value {
    let r = call(h, v);
    assert_eq!(r["ok"], true, "{r}");
    r["value"].clone()
}
fn state(h: &mut Helper, s: &str) -> Value {
    ok(h, json!({"op":"index_state","session":s}))
}
fn begin(h: &mut Helper, s: &str, r: &str, n: usize) {
    ok(
        h,
        json!({"op":"index_begin","session":s,"revision":r,"expected_documents":n}),
    );
}
fn import(h: &mut Helper, s: &str, r: &str, text: &str) {
    begin(h, s, r, 1);
    ok(
        h,
        json!({"op":"index_append","session":s,"documents":[{"seq":0,"text":text}]}),
    );
    ok(h, json!({"op":"index_commit","session":s}));
}
#[test]
fn lru_evicts_whole_sessions_and_queries_refresh_recency() {
    let f = Fixture::new();
    let mut h = f.helper();
    state(&mut h, "a");
    f.policy(1024, 2);
    import(&mut h, "a", "a1", "alpha");
    import(&mut h, "b", "b1", "beta");
    ok(
        &mut h,
        json!({"op":"search","session":"a","query":"no matches","limit":1}),
    );
    import(&mut h, "c", "c1", "gamma");
    assert_eq!(state(&mut h, "b"), Value::Null);
    assert_eq!(state(&mut h, "a")["revision"], "a1");
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","session":"b","query":"beta","limit":1})
        )["hits"],
        json!([])
    );
    ok(&mut h, json!({"op":"index_delete","session":"a"}));
    assert_eq!(state(&mut h, "a"), Value::Null);
}
#[test]
fn byte_quota_evicts_old_data_and_counts_unicode_and_empty_rows() {
    let f = Fixture::new();
    let mut h = f.helper();
    state(&mut h, "a");
    f.policy(270, 10);
    import(&mut h, "a", "1", "");
    import(&mut h, "b", "1", "");
    import(&mut h, "c", "1", "İİ");
    assert_eq!(state(&mut h, "a"), Value::Null);
    assert_eq!(state(&mut h, "b")["documents"], 1);
    let bytes: u64 = f
        .db()
        .query_row("SELECT bytes FROM sessions WHERE session='c'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(bytes, 138); // 4 input bytes + 6 folded bytes + row overhead.
}
#[test]
fn quota_failure_preserves_revision_and_other_sessions_then_can_abort() {
    let f = Fixture::new();
    let mut h = f.helper();
    state(&mut h, "a");
    f.policy(400, 10);
    import(&mut h, "a", "old", "old");
    import(&mut h, "b", "old", "peer");
    begin(&mut h, "a", "new", 1);
    let r = call(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"x".repeat(200)}]}),
    );
    assert_eq!(r["error"], "SESSION_INDEX_TEXT_BUDGET_EXCEEDED");
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["error"],
        "DOCUMENT_COUNT_MISMATCH"
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
    assert_eq!(state(&mut h, "b")["revision"], "old");
    ok(&mut h, json!({"op":"index_abort"}));
}
#[test]
fn commit_rechecks_policy_and_incremental_retained_bytes_atomically() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "a", "old", "old");
    import(&mut h, "b", "old", "peer");
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"new","expected_documents":1,"base_revision":"old","from_seq":1}),
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":1,"text":"tail"}]}),
    );
    f.policy(200, 10);
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["error"],
        "SESSION_INDEX_TEXT_BUDGET_EXCEEDED"
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
    assert_eq!(state(&mut h, "b")["revision"], "old");
}
#[test]
fn concurrent_replacement_or_eviction_cannot_commit_stale_import() {
    for evict in [false, true] {
        let f = Fixture::new();
        let mut first = f.helper();
        let mut second = f.helper();
        import(&mut first, "a", "old", "original");
        begin(&mut first, "a", "stale", 1);
        ok(
            &mut first,
            json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"stale"}]}),
        );
        if evict {
            f.policy(1024, 1);
            import(&mut second, "b", "new", "other");
        } else {
            import(&mut second, "a", "new", "replacement");
        }
        assert_eq!(
            call(&mut first, json!({"op":"index_commit","session":"a"}))["error"],
            "INDEX_REVISION_CHANGED_DURING_IMPORT"
        );
        assert_eq!(
            state(&mut first, "a"),
            if evict {
                Value::Null
            } else {
                json!({"revision":"new","documents":1})
            }
        );
    }
}
#[test]
fn import_above_old_32_mib_limit_is_disk_backed_and_batches_remain_bounded() {
    let f = Fixture::new();
    let mut h = f.helper();
    begin(&mut h, "large", "1", 34);
    for seq in 0..34 {
        ok(
            &mut h,
            json!({"op":"index_append","session":"large","documents":[{"seq":seq,"text":"a".repeat(1024*1024)}]}),
        );
    }
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"large"}))["documents"],
        34
    );
    begin(&mut h, "batch", "1", 3);
    let docs = (0..3)
        .map(|seq| json!({"seq":seq,"text":"x".repeat(1024*1024)}))
        .collect::<Vec<_>>();
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"batch","documents":docs})
        )["error"],
        "INDEX_BATCH_TOO_LARGE"
    );
    assert_eq!(state(&mut h, "large")["documents"], 34);
}
#[test]
fn old_v1_database_migrates_without_replacing_authoritative_revision() {
    let f = Fixture::new();
    {
        let db = f.db();
        db.execute_batch("CREATE TABLE sessions(session TEXT PRIMARY KEY,revision TEXT NOT NULL,documents INTEGER NOT NULL); CREATE TABLE documents(session TEXT NOT NULL,seq INTEGER NOT NULL,text TEXT NOT NULL,folded TEXT NOT NULL,PRIMARY KEY(session,seq)); INSERT INTO sessions VALUES('old','r1',1); INSERT INTO documents VALUES('old',0,'Hello','hello'); PRAGMA user_version=1;").unwrap();
    }
    let mut h = f.helper();
    assert_eq!(state(&mut h, "old"), json!({"revision":"r1","documents":1}));
    assert_eq!(
        f.db()
            .query_row("SELECT bytes FROM sessions WHERE session='old'", [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap(),
        138
    );
    import(&mut h, "old", "r2", "Updated");
    assert_eq!(state(&mut h, "old")["revision"], "r2");
}
#[test]
fn invalid_cache_policy_fails_closed_without_deleting_existing_data() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "a", "old", "original");
    f.policy(2 * 1024 * 1024 * 1024, 1);
    begin(&mut h, "a", "new", 1);
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"new"}]})
        )["error"],
        "INVALID_INDEX_CACHE_POLICY"
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
}
#[test]
fn failed_insert_rolls_back_evictions_and_original_replacement() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "a", "old", "original");
    import(&mut h, "b", "old", "peer");
    f.policy(200, 1);
    f.db().execute_batch("CREATE TRIGGER fail_replacement BEFORE INSERT ON documents WHEN new.text='reject' BEGIN SELECT RAISE(ABORT,'simulated write failure'); END;").unwrap();
    begin(&mut h, "a", "new", 1);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"reject"}]}),
    );
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["ok"],
        false
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
    assert_eq!(state(&mut h, "b")["revision"], "old");
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","session":"a","query":"original","limit":1})
        )["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn pinned_reader_bounds_wal_growth_and_retry_after_release_succeeds() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "a", "old", "original");
    let reader = f.db();
    reader
        .execute_batch("BEGIN; SELECT * FROM sessions;")
        .unwrap();
    begin(&mut h, "large", "1", 3);
    for seq in 0..3 {
        ok(
            &mut h,
            json!({"op":"index_append","session":"large","documents":[{"seq":seq,"text":"z".repeat(1024*1024)}]}),
        );
    }
    ok(&mut h, json!({"op":"index_commit","session":"large"}));
    let wal = f.0.join("desktop-text-v1.sqlite-wal");
    let size_before = std::fs::metadata(&wal).unwrap().len();
    assert!(size_before > 4 * 1024 * 1024);
    begin(&mut h, "next", "1", 1);
    ok(
        &mut h,
        json!({"op":"index_append","session":"next","documents":[{"seq":0,"text":"next"}]}),
    );
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"next"}))["error"],
        "INDEX_CACHE_CHECKPOINT_BUSY"
    );
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), size_before);
    assert_eq!(state(&mut h, "next"), Value::Null);
    reader.execute_batch("ROLLBACK").unwrap();
    ok(&mut h, json!({"op":"index_commit","session":"next"}));
    assert_eq!(state(&mut h, "next")["revision"], "1");
}
