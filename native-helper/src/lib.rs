use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

mod files;
mod index;
pub mod reader;
mod stream;

pub const PROTOCOL: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Request {
    pub id: u64,
    pub version: u32,
    #[serde(flatten)]
    pub operation: Operation,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Hello,
    ReadZstd {
        root: PathBuf,
        path: PathBuf,
        max_bytes: u64,
    },
    HashFile {
        root: PathBuf,
        path: PathBuf,
    },
    ReadSlice {
        root: PathBuf,
        path: PathBuf,
        offset: u64,
        length: usize,
    },
    IndexBegin {
        session: String,
        revision: String,
        expected_documents: u64,
        base_revision: Option<String>,
        from_seq: Option<u64>,
    },
    IndexAppend {
        session: String,
        documents: Vec<Document>,
    },
    IndexCommit {
        session: String,
    },
    IndexAbort,
    IndexState {
        session: String,
    },
    IndexDelete {
        session: String,
    },
    Search {
        query: String,
        session: Option<String>,
        limit: usize,
        expected_revision: Option<String>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub seq: u64,
    pub text: String,
    #[serde(default)]
    pub part: u32,
    #[serde(default)]
    pub preview: Option<String>,
}

pub struct Helper {
    index: Option<index::TextIndex>,
    cache: PathBuf,
}

impl Helper {
    pub fn open(cache: &Path) -> Result<Self, String> {
        if !cache.is_absolute() {
            return Err("CACHE_PATH_MUST_BE_ABSOLUTE".into());
        }
        Ok(Self {
            index: None,
            cache: cache.to_owned(),
        })
    }

    fn index(&mut self) -> Result<&mut index::TextIndex, String> {
        if self.index.is_none() {
            self.index = Some(index::TextIndex::open(&self.cache)?);
        }
        Ok(self.index.as_mut().unwrap())
    }

    pub fn execute(&mut self, request: Request) -> Value {
        let id = request.id;
        if request.version != PROTOCOL {
            return json!({"id": id, "ok": false, "error": "UNSUPPORTED_PROTOCOL"});
        }
        let result = match request.operation {
            Operation::ReadZstd { .. } => Err("STREAM_TRANSPORT_REQUIRED".into()),
            Operation::Hello => Ok(
                json!({"protocol": PROTOCOL, "index_schema": 1, "max_request_bytes": MAX_REQUEST_BYTES}),
            ),
            Operation::HashFile { root, path } => files::hash_file(&root, &path),
            Operation::ReadSlice {
                root,
                path,
                offset,
                length,
            } => files::read_slice(&root, &path, offset, length),
            Operation::IndexBegin {
                session,
                revision,
                expected_documents,
                base_revision,
                from_seq,
            } => self.index().and_then(|index| {
                index.begin(
                    &session,
                    &revision,
                    expected_documents,
                    base_revision.as_deref(),
                    from_seq,
                )
            }),
            Operation::IndexAppend { session, documents } => self
                .index()
                .and_then(|index| index.append(&session, documents)),
            Operation::IndexCommit { session } => {
                self.index().and_then(|index| index.commit(&session))
            }
            Operation::IndexAbort => self.index().and_then(|index| index.abort()),
            Operation::IndexState { session } => {
                self.index().and_then(|index| index.state(&session))
            }
            Operation::IndexDelete { session } => {
                self.index().and_then(|index| index.delete(&session))
            }
            Operation::Search {
                query,
                session,
                limit,
                expected_revision,
            } => self
                .index()
                .and_then(|index| index.search(&query, session.as_deref(), limit, expected_revision.as_deref())),
        };
        match result {
            Ok(value) => json!({"id": id, "ok": true, "value": value}),
            Err(error) => json!({"id": id, "ok": false, "error": error}),
        }
    }
}

/// Bounded NDJSON framing. Rejects an oversized frame before allocating it whole.
/// Oversized or invalid framing ends the connection so its remainder is never
/// misinterpreted as a new request. Closing a helper rolls back unfinished imports.
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    helper: &mut Helper,
) -> Result<(), String> {
    let mut frame = Vec::new();
    loop {
        let available = input.fill_buf().map_err(|e| e.to_string())?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(())
            } else {
                Err("TRUNCATED_REQUEST".into())
            };
        }
        let end = available.iter().position(|b| *b == b'\n');
        let length = end.map_or(available.len(), |i| i + 1);
        if frame.len() + length > MAX_REQUEST_BYTES {
            return Err("REQUEST_TOO_LARGE".into());
        }
        frame.extend_from_slice(&available[..length]);
        input.consume(length);
        if end.is_none() {
            continue;
        }
        let request: Request =
            serde_json::from_slice(&frame).map_err(|e| format!("INVALID_REQUEST: {e}"))?;
        let response = if let Operation::ReadZstd {
            root,
            path,
            max_bytes,
        } = &request.operation
        {
            if request.version != PROTOCOL {
                json!({"id":request.id,"ok":false,"error":"UNSUPPORTED_PROTOCOL"})
            } else {
                match stream::read_zstd(root, path, *max_bytes, request.id, &mut output) {
                    Ok(value) => json!({"id":request.id,"ok":true,"value":value}),
                    Err(error) => json!({"id":request.id,"ok":false,"error":error}),
                }
            }
        } else {
            helper.execute(request)
        };
        serde_json::to_writer(&mut output, &response).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        frame.clear();
    }
}
