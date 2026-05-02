main = () => {
    let ptr = __glyim_alloc(8) as *mut i64;
    *ptr = 42;
    *ptr
}
