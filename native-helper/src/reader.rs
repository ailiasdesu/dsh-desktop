//! Bounded pull reader shared with the Node-API adapter. Original logs are read
//! through an opened-handle containment check; partial/error results are never a
//! committed source. The caller must validate the final revision and semantics.
use crate::files::{open_scoped, unchanged};
use std::{
    fs::{File, Metadata},
    io::{BufRead, BufReader, Read},
    path::Path,
};

pub(crate) fn validate_frame_header(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 5 || bytes[..4] != [0x28, 0xb5, 0x2f, 0xfd] || bytes[4] & 0x18 != 0 {
        return Err("DSH_ZSTD_FRAME_HEADER_UNSUPPORTED".into());
    }
    Ok(())
}

pub struct Part {
    pub frame: u32,
    pub data: Vec<u8>,
    pub end: bool,
}
pub struct Batch {
    pub parts: Vec<Part>,
    pub done: bool,
    pub frames: u32,
    pub decoded_bytes: u64,
    pub compressed_bytes: u64,
}
pub struct ZstdReader {
    decoder: Option<zstd::stream::read::Decoder<'static, BufReader<File>>>,
    input: Option<BufReader<File>>,
    before: Metadata,
    total: u64,
    frames: u32,
    limit: u64,
    done: bool,
}
impl ZstdReader {
    pub fn open(root: &Path, path: &Path, limit: u64) -> Result<Self, String> {
        if limit == 0 || limit > 16 * 1024 * 1024 * 1024 {
            return Err("INVALID_DECODE_BUDGET".into());
        }
        let file = open_scoped(root, path)?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        Ok(Self {
            decoder: None,
            input: Some(BufReader::with_capacity(128 * 1024, file)),
            before,
            total: 0,
            frames: 0,
            limit,
            done: false,
        })
    }
    pub fn next(
        &mut self,
        max_bytes: usize,
        cancelled: impl Fn() -> bool,
    ) -> Result<Batch, String> {
        if max_bytes == 0 || max_bytes > 4 * 1024 * 1024 {
            return Err("INVALID_BATCH_BUDGET".into());
        }
        let mut parts = Vec::new();
        let mut bytes = 0;
        while !self.done && bytes < max_bytes && parts.len() < 256 {
            if cancelled() {
                return Err("NATIVE_ABORTED".into());
            }
            if self.decoder.is_none() {
                let mut input = self.input.take().ok_or("READER_CLOSED")?;
                if input.fill_buf().map_err(|e| e.to_string())?.is_empty() {
                    if self.frames == 0 {
                        return Err("EMPTY_ZSTD_STREAM".into());
                    }
                    unchanged(
                        &self.before,
                        &input.get_ref().metadata().map_err(|e| e.to_string())?,
                    )?;
                    self.done = true;
                    break;
                }
                validate_frame_header(input.fill_buf().map_err(|e| e.to_string())?)?;
                self.decoder = Some(
                    zstd::stream::read::Decoder::with_buffer(input)
                        .map_err(|e| e.to_string())?
                        .single_frame(),
                );
            }
            let mut data = vec![0u8; (max_bytes - bytes).min(128 * 1024)];
            let length = self
                .decoder
                .as_mut()
                .ok_or("READER_CLOSED")?
                .read(&mut data)
                .map_err(|e| format!("ZSTD_DECODE: {e}"))?;
            if length == 0 {
                self.input = Some(self.decoder.take().ok_or("READER_CLOSED")?.finish());
                parts.push(Part {
                    frame: self.frames,
                    data: Vec::new(),
                    end: true,
                });
                self.frames = self.frames.checked_add(1).ok_or("FRAME_COUNT_EXCEEDED")?;
            } else {
                self.total = self
                    .total
                    .checked_add(length as u64)
                    .ok_or("DECODE_BUDGET_EXCEEDED")?;
                if self.total > self.limit {
                    return Err("DECODE_BUDGET_EXCEEDED".into());
                }
                data.truncate(length);
                bytes += length;
                parts.push(Part {
                    frame: self.frames,
                    data,
                    end: false,
                });
            }
        }
        Ok(Batch {
            parts,
            done: self.done,
            frames: self.frames,
            decoded_bytes: self.total,
            compressed_bytes: self.before.len(),
        })
    }
}
