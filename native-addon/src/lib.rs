use dsh_native_helper::reader::{Batch, ZstdReader};
use napi::{bindgen_prelude::*, Env, Task};
use napi_derive::napi;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

struct State {
    reader: Option<ZstdReader>,
    opened: bool,
}

#[napi]
pub struct NativeReader {
    root: PathBuf,
    path: PathBuf,
    limit: u64,
    state: Arc<Mutex<State>>,
    cancelled: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

#[napi(object)]
pub struct Chunk {
    pub frame: u32,
    pub data: Buffer,
    pub end: bool,
}
#[napi(object)]
pub struct ReadBatch {
    pub parts: Vec<Chunk>,
    pub done: bool,
    pub frames: u32,
    pub decoded_bytes: f64,
    pub compressed_bytes: f64,
}

pub struct ReadTask {
    root: PathBuf,
    path: PathBuf,
    limit: u64,
    state: Arc<Mutex<State>>,
    cancelled: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

impl Task for ReadTask {
    type Output = Batch;
    type JsValue = ReadBatch;
    fn compute(&mut self) -> Result<Batch> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::from_reason("NATIVE_READER_POISONED"))?;
        if self.cancelled.load(Ordering::Acquire) {
            state.reader = None;
            return Err(Error::from_reason("NATIVE_ABORTED"));
        }
        if !state.opened {
            state.reader = Some(
                ZstdReader::open(&self.root, &self.path, self.limit).map_err(Error::from_reason)?,
            );
            state.opened = true;
        }
        let result = state
            .reader
            .as_mut()
            .ok_or_else(|| Error::from_reason("NATIVE_READER_CLOSED"))?
            .next(4 * 1024 * 1024, || self.cancelled.load(Ordering::Acquire));
        match result {
            Ok(batch) => {
                if batch.done {
                    state.reader = None;
                }
                Ok(batch)
            }
            Err(error) => {
                state.reader = None;
                Err(Error::from_reason(error))
            }
        }
    }
    fn resolve(&mut self, _env: Env, batch: Batch) -> Result<ReadBatch> {
        self.busy.store(false, Ordering::Release);
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::from_reason("NATIVE_ABORTED"));
        }
        Ok(ReadBatch {
            parts: batch
                .parts
                .into_iter()
                .map(|p| Chunk {
                    frame: p.frame,
                    data: p.data.into(),
                    end: p.end,
                })
                .collect(),
            done: batch.done,
            frames: batch.frames,
            decoded_bytes: batch.decoded_bytes as f64,
            compressed_bytes: batch.compressed_bytes as f64,
        })
    }
    fn reject(&mut self, _env: Env, error: Error) -> Result<ReadBatch> {
        self.busy.store(false, Ordering::Release);
        Err(error)
    }
    fn finally(self, _env: Env) -> Result<()> {
        self.busy.store(false, Ordering::Release);
        Ok(())
    }
}

#[napi]
impl NativeReader {
    #[napi(constructor)]
    pub fn new(root: String, path: String, max_bytes: f64) -> Result<Self> {
        if !max_bytes.is_finite()
            || max_bytes.fract() != 0.0
            || max_bytes < 1.0
            || max_bytes > 16.0 * 1024.0 * 1024.0 * 1024.0
        {
            return Err(Error::from_reason("INVALID_DECODE_BUDGET"));
        }
        Ok(Self {
            root: root.into(),
            path: path.into(),
            limit: max_bytes as u64,
            state: Arc::new(Mutex::new(State {
                reader: None,
                opened: false,
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
        })
    }
    #[napi]
    pub fn next(&self) -> Result<AsyncTask<ReadTask>> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::from_reason("NATIVE_READER_CLOSED"));
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(Error::from_reason("NATIVE_READER_BUSY"));
        }
        Ok(AsyncTask::new(ReadTask {
            root: self.root.clone(),
            path: self.path.clone(),
            limit: self.limit,
            state: self.state.clone(),
            cancelled: self.cancelled.clone(),
            busy: self.busy.clone(),
        }))
    }
    #[napi]
    pub fn close(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.try_lock() {
            state.reader = None;
        }
    }
}

#[napi]
pub fn protocol_version() -> u32 {
    1
}
