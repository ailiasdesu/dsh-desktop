use dsh_native_helper::{serve, Helper, Request, MAX_REQUEST_BYTES};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let path = base.join(format!(
            "dsh-native-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn helper(&self) -> Helper {
        Helper::open(&self.0.join("cache")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let base = std::env::temp_dir().canonicalize().unwrap();
        if self.0.parent() == Some(base.as_path())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dsh-native-test-")
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn call(h: &mut Helper, mut operation: Value) -> Value {
    operation["id"] = json!(1);
    operation["version"] = json!(1);
    let request: Request = serde_json::from_value(operation).unwrap();
    h.execute(request)
}
fn ok(h: &mut Helper, operation: Value) -> Value {
    let result = call(h, operation);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn import(h: &mut Helper, revision: &str, docs: Value) {
    ok(
        h,
        json!({"op":"index_begin","session":"a","revision":revision,"expected_documents":docs.as_array().unwrap().len()}),
    );
    ok(
        h,
        json!({"op":"index_append","session":"a","documents":docs}),
    );
    ok(h, json!({"op":"index_commit","session":"a"}));
}

#[test]
fn protocol_roundtrip_and_version_refusal() {
    let f = Fixture::new();
    let mut h = f.helper();
    let mut out = Vec::new();
    serve(
        Cursor::new(b"{\"id\":7,\"version\":1,\"op\":\"hello\"}\n"),
        &mut out,
        &mut h,
    )
    .unwrap();
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["value"]["protocol"], 1);
    let request = serde_json::from_value(json!({"id":8,"version":2,"op":"hello"})).unwrap();
    assert_eq!(h.execute(request)["error"], "UNSUPPORTED_PROTOCOL");
}

#[test]
fn bounded_and_truncated_framing_fail_closed() {
    let f = Fixture::new();
    let mut h = f.helper();
    assert_eq!(
        serve(
            Cursor::new(vec![b' '; MAX_REQUEST_BYTES + 1]),
            Vec::new(),
            &mut h
        )
        .unwrap_err(),
        "REQUEST_TOO_LARGE"
    );
    assert_eq!(
        serve(Cursor::new(b"{"), Vec::new(), &mut h).unwrap_err(),
        "TRUNCATED_REQUEST"
    );
}

#[test]
fn file_hash_empty_unicode_and_scope() {
    let f = Fixture::new();
    let mut h = f.helper();
    std::fs::write(f.0.join("empty"), b"").unwrap();
    let hash = ok(&mut h, json!({"op":"hash_file","root":f.0,"path":"empty"}));
    assert_eq!(
        hash["sha256"],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    std::fs::write(f.0.join("中文.txt"), "hello 世界").unwrap();
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"read_slice","root":f.0,"path":"中文.txt","offset":6,"length":6})
        )["text"],
        "世界"
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"read_slice","root":f.0,"path":"中文.txt","offset":7,"length":2})
        )["ok"],
        false
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"hash_file","root":f.0,"path":"../outside"})
        )["error"],
        "INVALID_SCOPED_PATH"
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"read_slice","root":f.0,"path":"empty","offset":0,"length":999999})
        )["error"],
        "SLICE_TOO_LARGE"
    );
}

#[test]
fn replacement_is_atomic_and_survives_restart() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(&mut h, "r1", json!([{"seq":1,"text":"old apple"}]));
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"r2","expected_documents":2}),
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":1,"text":"new pear"}]}),
    );
    assert_eq!(
        call(&mut h, json!({"op":"index_commit","session":"a"}))["error"],
        "DOCUMENT_COUNT_MISMATCH"
    );
    assert_eq!(
        ok(&mut h, json!({"op":"search","query":"apple","limit":10}))["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(h);
    let mut h = f.helper();
    assert_eq!(
        ok(&mut h, json!({"op":"index_state","session":"a"}))["revision"],
        "r1"
    );
    import(&mut h, "r2", json!([{"seq":1,"text":"new pear"}]));
    assert!(
        ok(&mut h, json!({"op":"search","query":"apple","limit":10}))["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn repeat_queries_unicode_literals_and_global_limit() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(
        &mut h,
        "r1",
        json!([{"seq":1,"text":"中文 APPLE 100% _ '"},{"seq":3,"text":"中文 apple"},{"seq":5,"text":"pear"}]),
    );
    for query in ["中文", "apple", "APPLE"] {
        let result = ok(&mut h, json!({"op":"search","query":query,"limit":1}));
        assert_eq!(result["hits"].as_array().unwrap().len(), 1);
        assert_eq!(result["has_more"], true);
    }
    assert_eq!(
        ok(&mut h, json!({"op":"search","query":"% _ '","limit":10}))["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(
            &mut h,
            json!({"op":"search","query":"pear","session":"b","limit":10})
        )["hits"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    ok(&mut h, json!({"op":"index_delete","session":"a"}));
    assert!(ok(&mut h, json!({"op":"index_state","session":"a"})).is_null());
}

#[test]
fn import_rejects_out_of_order_without_partial_batch() {
    let f = Fixture::new();
    let mut h = f.helper();
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"r1","expected_documents":2}),
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":5,"text":"x"},{"seq":4,"text":"y"}]})
        )["error"],
        "DOCUMENT_SEQUENCE_INVALID"
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":4,"text":"x"},{"seq":5,"text":"y"}]}),
    );
    ok(&mut h, json!({"op":"index_commit","session":"a"}));
}

#[test]
fn incremental_revision_checks_and_suffix_replacement() {
    let f = Fixture::new();
    let mut h = f.helper();
    import(
        &mut h,
        "r1",
        json!([{"seq":1,"text":"keep"},{"seq":3,"text":"replace"}]),
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_begin","session":"a","revision":"r2","base_revision":"wrong","from_seq":3,"expected_documents":1})
        )["error"],
        "INDEX_REVISION_MISMATCH"
    );
    ok(
        &mut h,
        json!({"op":"index_begin","session":"a","revision":"r2","base_revision":"r1","from_seq":3,"expected_documents":1}),
    );
    assert_eq!(
        call(
            &mut h,
            json!({"op":"index_append","session":"a","documents":[{"seq":1,"text":"bad"}]})
        )["error"],
        "DOCUMENT_BEFORE_INCREMENTAL_BASE"
    );
    ok(
        &mut h,
        json!({"op":"index_append","session":"a","documents":[{"seq":5,"text":"added"}]}),
    );
    assert_eq!(
        ok(&mut h, json!({"op":"index_commit","session":"a"}))["documents"],
        2
    );
    for (query, count) in [("keep", 1), ("replace", 0), ("added", 1)] {
        assert_eq!(
            ok(&mut h, json!({"op":"search","query":query,"limit":10}))["hits"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
}

#[test]
fn concurrent_cache_writer_cannot_publish_over_a_changed_revision() {
    let f = Fixture::new();
    let mut first = f.helper();
    import(&mut first, "r1", json!([{"seq":1,"text":"old"}]));
    let mut second = f.helper();
    ok(
        &mut first,
        json!({"op":"index_begin","session":"a","revision":"stale","expected_documents":0}),
    );
    import(&mut second, "r2", json!([{"seq":1,"text":"new"}]));
    assert_eq!(
        call(&mut first, json!({"op":"index_commit","session":"a"}))["error"],
        "INDEX_REVISION_CHANGED_DURING_IMPORT"
    );
    assert_eq!(
        ok(&mut first, json!({"op":"index_state","session":"a"}))["revision"],
        "r2"
    );
}

#[test]
fn hello_and_file_operations_do_not_open_the_index_database() {
    let f = Fixture::new();
    let mut h = f.helper();
    ok(&mut h, json!({"op":"hello"}));
    assert!(!f.0.join("cache").exists());
    std::fs::write(f.0.join("tiny"), "x").unwrap();
    ok(&mut h, json!({"op":"hash_file","root":f.0,"path":"tiny"}));
    assert!(!f.0.join("cache").exists());
}

#[test]
fn future_database_schema_is_refused_without_overwriting_it() {
    let f = Fixture::new();
    std::fs::create_dir(f.0.join("cache")).unwrap();
    let db = rusqlite::Connection::open(f.0.join("cache/desktop-text-v1.sqlite")).unwrap();
    db.execute_batch("PRAGMA user_version=99; CREATE TABLE sentinel (value TEXT); INSERT INTO sentinel VALUES ('preserved');").unwrap();
    let mut h = f.helper();
    assert_eq!(
        call(&mut h, json!({"op":"index_state","session":"a"}))["error"],
        "UNSUPPORTED_INDEX_SCHEMA"
    );
    assert_eq!(
        db.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
}
