use core::slice;
use std::ffi::CStr;
use std::io::Write;
use std::fs;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_cov_flush_impl(
    counters: *const i64,
    counters_len: u64,
    _dump_json: *const u8,
    _dump_json_len: u64,
    out_path: *const u8,
) {
    if counters.is_null() || out_path.is_null() {
        return;
    }
    let counts = unsafe { slice::from_raw_parts(counters, counters_len as usize) };
    let path = unsafe { CStr::from_ptr(out_path as *const i8) }.to_string_lossy();
    let mut file = match fs::File::create(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return,
    };
    // Write JSON manually
    let _ = write!(file, "{{\"counters\":[");
    for (i, &count) in counts.iter().enumerate() {
        if i > 0 {
            let _ = write!(file, ",");
        }
        let _ = write!(file, "{}", count);
    }
    let _ = write!(file, "],\"version\":1}}");
}
