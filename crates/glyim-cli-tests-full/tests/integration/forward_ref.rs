#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_forward_reference_struct_and_impl() {
    // Iter::new is called in Vec::iter, but Iter is defined after Vec
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
fn e2e_forward_reference_for_loop_vec() {
    // Same as e2e_for_loop_vec but with Iter before Vec
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

