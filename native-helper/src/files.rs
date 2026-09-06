use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: usize = 128 * 1024;

fn open_scoped(root: &Path, path: &Path) -> Result<File, String> {
    if !root.is_absolute()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("INVALID_SCOPED_PATH".into());
    }
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = canonical_root
        .join(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !target.starts_with(&canonical_root) {
        return Err("PATH_OUTSIDE_ROOT".into());
    }
    let file = File::open(target).map_err(|e| e.to_string())?;
    verify_open_handle(&file, &canonical_root)?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("NOT_A_FILE".into());
    }
    Ok(file)
}

// Resolve the opened handle, closing the canonicalize/open reparse race on
// Windows. The host still owns tool authorization; this is only path scope.
#[cfg(windows)]
fn verify_open_handle(file: &File, root: &Path) -> Result<(), String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let handle = file.as_raw_handle();
    let length = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
    if length == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let actual =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if actual == 0 || actual as usize >= buffer.len() {
        return Err("FILE_HANDLE_PATH_UNAVAILABLE".into());
    }
    let target =
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..actual as usize]));
    if !target.starts_with(root) {
        return Err("PATH_OUTSIDE_ROOT".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_open_handle(_file: &File, _root: &Path) -> Result<(), String> {
    Err("FILE_OPERATIONS_NOT_VALIDATED_ON_THIS_PLATFORM".into())
}

fn unchanged(before: &Metadata, after: &Metadata) -> Result<(), String> {
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        Err("FILE_CHANGED_DURING_READ".into())
    } else {
        Ok(())
    }
}

pub fn hash_file(root: &Path, path: &Path) -> Result<Value, String> {
    let mut file = open_scoped(root, path)?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    let mut bytes = 0_u64;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    unchanged(&before, &file.metadata().map_err(|e| e.to_string())?)?;
    if bytes != before.len() {
        return Err("FILE_CHANGED_DURING_READ".into());
    }
    Ok(json!({"sha256": format!("{:x}", hash.finalize()), "bytes": bytes}))
}

pub fn read_slice(root: &Path, path: &Path, offset: u64, length: usize) -> Result<Value, String> {
    if length > CHUNK {
        return Err("SLICE_TOO_LARGE".into());
    }
    let mut file = open_scoped(root, path)?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if offset > before.len() {
        return Err("OFFSET_PAST_END".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut data = vec![0; length.min((before.len() - offset) as usize)];
    file.read_exact(&mut data).map_err(|e| e.to_string())?;
    unchanged(&before, &file.metadata().map_err(|e| e.to_string())?)?;
    // This primitive is for bounded text previews, never a replacement for
    // binary attachments; invalid UTF-8 is a refusal, not lossy corruption.
    let text = String::from_utf8(data).map_err(|_| "SLICE_NOT_UTF8")?;
    Ok(json!({"text": text, "offset": offset, "bytes": text.len(), "file_bytes": before.len()}))
}
