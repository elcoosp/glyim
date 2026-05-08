use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::io::Write;
use std::slice;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceLocation {
    file_id: u32,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
    kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileInfo {
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoverageDump {
    files: HashMap<u32, FileInfo>,
    counters: HashMap<u64, i64>,
    metadata: HashMap<u64, SourceLocation>,
    version: u32,
}

/// # Safety
///
/// The caller must ensure that `counters`, `dump_json`, and `out_path` are valid non-null pointers
/// pointing to valid memory of the specified lengths. This function will read from these pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_cov_flush_impl(
    counters: *const i64,
    counters_len: u64,
    dump_json: *const u8,
    dump_json_len: u64,
    out_path: *const u8,
) {
    if counters.is_null() || dump_json.is_null() || out_path.is_null() {
        return;
    }
    let counts = unsafe { slice::from_raw_parts(counters, counters_len as usize) };
    let json_bytes = unsafe { slice::from_raw_parts(dump_json, dump_json_len as usize) };
    let mut dump: CoverageDump = match serde_json::from_slice(json_bytes) {
        Ok(d) => d,
        Err(_) => return,
    };
    for (id, &count) in counts.iter().enumerate() {
        dump.counters.insert(id as u64, count);
    }
    let data = match serde_json::to_string(&dump) {
        Ok(s) => s,
        Err(_) => return,
    };
    let path = unsafe { CStr::from_ptr(out_path as *const i8) }.to_string_lossy();
    let mut file = match fs::File::create(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return,
    };
    let _ = file.write_all(data.as_bytes());
}
