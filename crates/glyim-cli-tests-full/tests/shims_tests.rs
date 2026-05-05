use glyim_cli::pipeline;

#[test]
fn test_alloc_free_roundtrip() {
    let src = r#"
        main = () => {
            let ptr = __glyim_alloc(16);
            let typed = ptr as *mut i64;
            *typed = 99;
            let val = *typed;
            __glyim_free(ptr as *mut u8);
            val
        }
    "#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 99);
}

#[test]
fn test_alloc_zero_size() {
    let src = r#"
        main = () => {
            let ptr = __glyim_alloc(0);
            if ptr == (0 as *mut u8) { 0 } else { 1 }
        }
    "#;
    let result = pipeline::run_jit(src).unwrap();
    // malloc(0) may return null or a valid pointer; either is acceptable
    assert!(result == 0 || result == 1);
}

#[test]
fn test_hash_bytes_deterministic() {
    let src = r#"
        main = () => {
            let data = "hello" as *const u8;
            __glyim_hash_bytes(data, 5)
        }
    "#;
    let result1 = pipeline::run_jit(src).unwrap();
    let result2 = pipeline::run_jit(src).unwrap();
    assert_eq!(result1, result2, "hash should be deterministic");
}
