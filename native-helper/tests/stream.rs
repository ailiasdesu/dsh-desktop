use base64::{engine::general_purpose::STANDARD, Engine};
use dsh_native_helper::{serve, Helper};
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
            "dsh-zstd-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn decode(&self, bytes: &[u8], limit: u64) -> Vec<Value> {
        std::fs::write(self.0.join("log.zstd"), bytes).unwrap();
        let request = json!({"id":1,"version":1,"op":"read_zstd","root":self.0,"path":"log.zstd","max_bytes":limit});
        let mut helper = Helper::open(&self.0.join("cache")).unwrap();
        let mut output = Vec::new();
        serve(
            Cursor::new(format!("{request}\n")),
            &mut output,
            &mut helper,
        )
        .unwrap();
        let mut messages = Vec::new();
        let mut start = 0;
        while start < output.len() {
            let end = start + output[start..].iter().position(|b| *b == b'\n').unwrap();
            let mut value: Value = serde_json::from_slice(&output[start..end]).unwrap();
            start = end + 1;
            if let Some(bytes) = value["binary"].as_u64() {
                let end = start + bytes as usize;
                value["progress"] =
                    json!({"frame":value["frame"],"data":STANDARD.encode(&output[start..end])});
                start = end;
            }
            messages.push(value);
        }
        messages
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.parent() == Some(std::env::temp_dir().canonicalize().unwrap().as_path())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dsh-zstd-test-")
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn frame(data: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(Cursor::new(data), 1).unwrap()
}
#[test]
fn multiframe_chunks_preserve_all_bytes_and_bound_output() {
    let f = Fixture::new();
    let text = "中文 hello\n".repeat(50000);
    let mut bytes = frame(b"{\"type\":\"session\"}\n");
    bytes.extend(frame(text.as_bytes()));
    let result = f.decode(&bytes, 10_000_000);
    let final_result = result.last().unwrap();
    assert_eq!(final_result["ok"], true);
    assert_eq!(final_result["value"]["frames"], 2);
    let mut decoded = Vec::new();
    for message in &result {
        if let Some(data) = message["progress"]["data"].as_str() {
            let chunk = STANDARD.decode(data).unwrap();
            assert!(chunk.len() <= 128 * 1024);
            decoded.extend(chunk);
        }
    }
    assert_eq!(
        decoded,
        [b"{\"type\":\"session\"}\n".as_slice(), text.as_bytes()].concat()
    );
}
#[test]
fn incomplete_or_corrupt_frames_never_report_success() {
    let f = Fixture::new();
    let mut bytes = frame(b"header\n");
    let last = frame(&vec![b'a'; 200000]);
    bytes.extend(&last[..last.len() - 2]);
    assert_eq!(f.decode(&bytes, 1_000_000).last().unwrap()["ok"], false);
    let mut bytes = frame(b"header\n");
    bytes.extend(b"broken middle");
    bytes.extend(frame(b"tail\n"));
    assert_eq!(f.decode(&bytes, 1_000_000).last().unwrap()["ok"], false);
}
#[test]
fn budgets_and_empty_input_refuse_without_partial_success() {
    let f = Fixture::new();
    assert_eq!(
        f.decode(&frame(&vec![b'x'; 200000]), 1000).last().unwrap()["error"],
        "DECODE_BUDGET_EXCEEDED"
    );
    assert_eq!(
        f.decode(b"", 1000).last().unwrap()["error"],
        "EMPTY_ZSTD_STREAM"
    );
}
