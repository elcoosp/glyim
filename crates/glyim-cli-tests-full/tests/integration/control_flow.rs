#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_block_last() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { 1 2 }"), None).unwrap(),
        2
    );
}

#[test]
fn e2e_if_true_branch() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if true { 10 } else { 20 } }"), None).unwrap(),
        10
    );
}

#[test]
fn e2e_if_false_branch() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { if false { 10 } else { 20 } }"),
            None
        )
        .unwrap(),
        20
    );
}

#[test]
fn e2e_if_without_else() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if false { 42 } }"), None).unwrap(),
        0
    );
}

#[test]
fn e2e_else_if_chain() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { if false { 1 } else if false { 2 } else { 3 } }"),
            None
        )
        .unwrap(),
        3
    );
}

#[test]
fn e2e_match() {
    assert_eq!(pipeline::run(&temp_g("enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Red; match c { Color::Red => 1, Color::Green => 2, Color::Blue => 3 } }"), None).unwrap(), 1);
}

#[test]
fn e2e_arrow() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let r = Ok(42)?; r }"), None).unwrap(),
        42
    );
}

#[test]
fn e2e_while_loop() {
    let src = r#"
main = () => {
    let mut i = 0;
    let mut sum = 0;
    while i < 5 {
        sum = sum + i;
        i = i + 1;
    };
    sum
}
"#;
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 10);
}

#[test]
fn e2e_for_in_vec() {
    let iter_src = include_str!("../../../../stdlib/src/iter.g");
    let vec_src = include_str!("../../../../stdlib/src/vec.g");
    let main_code = r#"
main = () => {
    let v: Vec<i64> = Vec::new();
    let v = v.push(10);
    let v = v.push(20);
    let v = v.push(30);
    let mut sum = 0;
    for x in v.iter() {
        sum = sum + x
    };
    sum
}
"#;
    let full_src = format!("{}\n{}\n{}", iter_src, vec_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 60); // 10+20+30
}

#[test]
fn e2e_for_in_range() {
    let range_src = include_str!("../../../../stdlib/src/range.g");
    let main_code = r#"
main = () => {
    let mut sum = 0;
    for i in Range::new(1, 5) {
        sum = sum + i
    };
    sum
}
"#;
    let full_src = format!("{}\n{}", range_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 10); // 1+2+3+4
}

#[test]
fn e2e_for_loop_range() {
    let range_src = include_str!("../../../../stdlib/src/range.g");
    let main_code = r#"
main = () => {
    let r = Range::new(0, 3);
    let mut sum = 0;
    for i in r {
        sum = sum + i
    };
    sum
}
"#;
    let full_src = format!("{}\n{}", range_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 3);
}

#[test]
fn e2e_for_loop_vec() {
    let iter_src = include_str!("../../../../stdlib/src/iter.g");
    let vec_src = include_str!("../../../../stdlib/src/vec.g");
    let main_code = r#"
main = () => {
    let v: Vec<i64> = Vec::new();
    let v = v.push(10);
    let v = v.push(20);
    let v = v.push(30);
    let mut sum = 0;
    for x in v.iter() {
        sum = sum + x
    };
    sum
}
"#;
    let full_src = format!("{}\n{}\n{}", iter_src, vec_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 60);
}

#[test]
fn e2e_for_loop_iter_simple() {
    let iter_src = include_str!("../../../../stdlib/src/iter.g");
    let vec_src = include_str!("../../../../stdlib/src/vec.g");
    let main_code = r#"
main = () => {
    let v: Vec<i64> = Vec::new();
    let v = v.push(10);
    let it = v.iter();
    let val = it.next();
    match val {
        Some(x) => x,
        None => 0,
    }
}
"#;
    let full_src = format!("{}\n{}\n{}", iter_src, vec_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 10);
}

#[test]
fn e2e_iter_next_direct() {
    let iter_src = include_str!("../../../../stdlib/src/iter.g");
    let main_code = r#"
main = () => {
    // Allocate a single i64 on the heap via the alloc shim
    let ptr = __glyim_alloc(8) as *mut i64;
    *ptr = 99;
    let it: Iter<i64> = Iter::new(ptr, 1);
    let val = it.next();
    match val {
        Some(x) => x,
        None => 0,
    }
}
"#;
    let full_src = format!("{}\n{}", iter_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 99);
}

#[test]
fn e2e_range_for_loop_sum() {
    let range_src = include_str!("../../../../stdlib/src/range.g");
    let main_code = r#"
main = () => {
    let mut sum = 0;
    for i in Range::new(1, 5) {
        sum = sum + i
    };
    sum
}
"#;
    let full_src = format!("{}\n{}", range_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input, None).unwrap(), 10);
}

#[test]
#[ignore = "codegen bug: enum tuple variant field binding returns 0"]
fn e2e_match_tuple_variant_bind() {
    let src = "enum Color { RGB(i64, i64, i64) }
main = () => { let c = Color::RGB(1,2,3); match c { Color::RGB(r,g,b) => r+g+b } }";
    let result = pipeline::run_jit(src);
    assert!(
        result.is_ok(),
        "match tuple variant bind: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), 6);
}

#[test]
fn e2e_match_guard() {
    let src = "main = () => { let v = Some(42); match v { Some(x) if x > 40 => 1, _ => 0 } }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "match guard: {:?}", result.err());
    assert_eq!(result.unwrap(), 1);
}

