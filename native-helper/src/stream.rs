use crate::files::{open_scoped, unchanged};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::Path;

const CHUNK: usize = 128 * 1024;

fn progress(output: &mut impl Write, id: u64, value: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, &json!({"id":id,"ok":true,"progress":value}))
        .map_err(|e| e.to_string())?;
    output.write_all(b"\n").map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())
}

/// Decode structurally delimited frames. Never scan compressed payload for magic
/// bytes or suppress checksum/truncation errors. Consumers publish only after
/// the final success envelope and their own official revision/semantic checks.
pub(crate) fn read_zstd(
    root: &Path,
    path: &Path,
    max_bytes: u64,
    id: u64,
    output: &mut impl Write,
) -> Result<Value, String> {
    if max_bytes == 0 || max_bytes > 16 * 1024 * 1024 * 1024 {
        return Err("INVALID_DECODE_BUDGET".into());
    }
    let file = open_scoped(root, path)?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    let mut input = BufReader::with_capacity(CHUNK, file);
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0u64;
    let mut frames = 0u64;
    let mut context = zstd::zstd_safe::DCtx::create();
    while !input.fill_buf().map_err(|e| e.to_string())?.is_empty() {
        crate::reader::validate_frame_header(input.fill_buf().map_err(|e| e.to_string())?)?;
        let start = input.stream_position().map_err(|e| e.to_string())?;
        let mut decoder =
            zstd::stream::read::Decoder::with_context(input, &mut context).single_frame();
        loop {
            let bytes = decoder
                .read(&mut buffer)
                .map_err(|e| format!("ZSTD_DECODE: {e}"))?;
            if bytes == 0 {
                break;
            }
            total = total
                .checked_add(bytes as u64)
                .ok_or("DECODE_BUDGET_EXCEEDED")?;
            if total > max_bytes {
                return Err("DECODE_BUDGET_EXCEEDED".into());
            }
            serde_json::to_writer(
                &mut *output,
                &json!({"id":id,"ok":true,"binary":bytes,"frame":frames}),
            )
            .map_err(|e| e.to_string())?;
            output.write_all(b"\n").map_err(|e| e.to_string())?;
            output
                .write_all(&buffer[..bytes])
                .map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
        }
        input = decoder.finish();
        let end = input.stream_position().map_err(|e| e.to_string())?;
        if end <= start {
            return Err("ZSTD_NO_PROGRESS".into());
        }
        progress(
            output,
            id,
            json!({"frame":frames,"end":true,"compressed_start":start,"compressed_end":end}),
        )?;
        frames += 1;
    }
    if frames == 0 {
        return Err("EMPTY_ZSTD_STREAM".into());
    }
    unchanged(
        &before,
        &input.get_ref().metadata().map_err(|e| e.to_string())?,
    )?;
    Ok(json!({"frames":frames,"decoded_bytes":total,"compressed_bytes":before.len()}))
}
