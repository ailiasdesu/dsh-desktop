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
            "dsh-document-chunks-{}-{}",
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
                .starts_with("dsh-document-chunks-")
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
fn overlap_matches_cross_boundary_once_and_preserves_original_preview() {
    let f = Fixture::new();
    let mut h = f.helper();
    begin(&mut h, "a", "r1", 3);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[
        {"seq":0,"part":0,"text":"start cross","preview":"Original UPPERCASE preview"},
        {"seq":0,"part":1,"text":"cross boundary end","preview":"Original UPPERCASE preview"},
        {"seq":1,"part":0,"text":"cross boundary second"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"}))["documents"],
        2
    );
    let result = ok(
        &mut h,
        json!({"op":"search","session":"a","query":"cross boundary","limit":1}),
    );
    assert_eq!(result["hits"].as_array().unwrap().len(), 1);
    assert_eq!(result["hits"][0]["preview"], "Original UPPERCASE preview");
    assert_eq!(result["has_more"], true);
    let cross = ok(
        &mut h,
        json!({"op":"search","session":"a","query":"cross","limit":10}),
    );
    assert_eq!(cross["hits"].as_array().unwrap().len(), 2);
    assert_eq!(cross["has_more"], false);
}
#[test]
fn large_logical_document_spans_bounded_batches_and_reopens() {
    let f = Fixture::new();
    {
        let mut h = f.helper();
        begin(&mut h, "large", "r1", 20);
        for part in 0..20 {
            ok(
                &mut h,
                json!({"op":"index_append","session":"large","documents":[{"seq":42,"part":part,"text":"abc".repeat(32768),"preview":"Full message beginning"}]}),
            );
        }
        assert_eq!(
            ok(&mut h, json!({"op":"index_commit","session":"large"}))["documents"],
            1
        );
    }
    let mut h = f.helper();
    let result = ok(
        &mut h,
        json!({"op":"search","session":"large","query":"abc","limit":10}),
    );
    assert_eq!(result["hits"].as_array().unwrap().len(), 1);
    assert_eq!(result["hits"][0]["seq"], 42);
    assert_eq!(result["hits"][0]["preview"], "Full message beginning");
    assert_eq!(result["has_more"], false);
}
#[test]
fn gaps_duplicates_and_nonzero_first_parts_fail_without_partial_batch() {
    let f = Fixture::new();
    let mut h = f.helper();
    begin(&mut h, "a", "r1", 3);
    for docs in [
        json!([{"seq":0,"part":1,"text":"bad"}]),
        json!([{"seq":0,"part":0,"text":"a"},{"seq":0,"part":2,"text":"bad"}]),
        json!([{"seq":0,"part":0,"text":"a"},{"seq":0,"part":0,"text":"bad"}]),
        json!([{"seq":0,"part":0,"text":"a"},{"seq":1,"part":1,"text":"bad"}]),
    ] {
        assert_eq!(
            call(
                &mut h,
                json!({"op":"index_append","session":"a","documents":docs})
            )["error"],
            "DOCUMENT_PART_INVALID"
        );
    }
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"part":0,"text":"a"}]}),
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":0,"part":0,"text":"duplicate"}]})
        )["error"],
        "DOCUMENT_PART_INVALID"
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"part":1,"text":"b"},{"seq":1,"part":0,"text":"c"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"}))["documents"],
        2
    );
}
#[test]
fn incremental_replace_removes_all_old_tail_parts() {
    let f = Fixture::new();
    let mut h = f.helper();
    begin(&mut h, "a", "r1", 3);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"keep"},{"seq":5,"part":0,"text":"old"},{"seq":5,"part":1,"text":"stale"}]}),
    );
    ok(&mut h, json!({"op":"index_commit","session":"a"}));
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"r2","base_revision":"r1","from_seq":5,"expected_documents":1}),
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":5,"text":"new"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"}))["documents"],
        2
    );
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","session":"a","query":"stale","limit":10})
        )["hits"],
        json!([])
    );
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","session":"a","query":"keep","limit":10})
        )["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn preview_bytes_are_charged_and_oversized_preview_is_rejected() {
    let f = Fixture::new();
    let mut h = f.helper();
    state(&mut h, "a");
    f.policy(140, 10);
    begin(&mut h, "a", "r1", 1);
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"a","preview":"large preview"}]})
        )["error"],
        "SESSION_INDEX_TEXT_BUDGET_EXCEEDED"
    );
    f.policy(4096, 10);
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"a","preview":"p".repeat(241)}]})
        )["error"],
        "DOCUMENT_PREVIEW_TOO_LARGE"
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"a","preview":"😀"}]}),
    );
    ok(&mut h, json!({"op":"index_commit","session":"a"}));
    assert_eq!(
        f.db()
            .query_row("SELECT bytes FROM sessions WHERE session='a'", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        134
    );
}
#[test]
fn incomplete_chunk_import_preserves_old_index_and_atomic_retry() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "a", "old", "original");
    begin(&mut h, "a", "new", 2);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"part":0,"text":"new start"}]}),
    );
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["error"],
        "DOCUMENT_COUNT_MISMATCH"
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"part":1,"text":"new tail"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"})),
        json!({"revision":"new","documents":1})
    );
}
#[test]
fn incremental_quota_charges_retained_preview_and_all_tail_parts() {
    let f = Fixture::new();
    let mut h = f.helper();
    begin(&mut h, "a", "old", 2);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":0,"text":"a","preview":"p".repeat(100)},{"seq":1,"text":"b"}]}),
    );
    ok(&mut h, json!({"op":"index_commit","session":"a"}));
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"new","expected_documents":2,"base_revision":"old","from_seq":1}),
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":1,"part":0,"text":"c","preview":"origin"},{"seq":1,"part":1,"text":"d","preview":"origin"}]}),
    );
    f.policy(450, 10);
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["error"],
        "SESSION_INDEX_TEXT_BUDGET_EXCEEDED"
    );
    assert_eq!(state(&mut h, "a")["revision"], "old");
    f.policy(512, 10);
    ok(&mut h, json!({"op":"index_commit","session":"a"}));
    assert_eq!(
        f.db()
            .query_row("SELECT bytes FROM sessions WHERE session='a'", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        502
    );
}
#[test]
fn cache_with_existing_quota_metadata_migrates_legacy_primary_key() {
    let f = Fixture::new();
    {
        let db = f.db();
        db.execute_batch("CREATE TABLE sessions(session TEXT PRIMARY KEY,revision TEXT NOT NULL,documents INTEGER NOT NULL,bytes INTEGER NOT NULL,last_used INTEGER NOT NULL); CREATE TABLE documents(session TEXT NOT NULL,seq INTEGER NOT NULL,text TEXT NOT NULL,folded TEXT NOT NULL,PRIMARY KEY(session,seq)); INSERT INTO sessions VALUES('a','legacy',1,138,9); INSERT INTO documents VALUES('a',4,'Hello','hello'); PRAGMA user_version=1;").unwrap();
    }
    let mut h = f.helper();
    assert_eq!(state(&mut h, "a")["revision"], "legacy");
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","session":"a","query":"hello","limit":1})
        )["hits"][0]["preview"],
        "Hello"
    );
    begin(&mut h, "a", "chunked", 2);
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":4,"part":0,"text":"new"},{"seq":4,"part":1,"text":"tail"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"}))["documents"],
        1
    );
}
