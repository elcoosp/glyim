main = () => {
    let p = glyim_alloc(8) as *mut i64
    let offset = __ptr_offset(p as *mut u8, 1 as i64)
    let result = offset as i64 - p as i64
    glyim_free(p as *mut u8)
    result
}
