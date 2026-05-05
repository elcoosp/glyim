#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_enum() {
    assert_eq!(
        pipeline::run(
            &temp_g("enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Green; 1 }"),
            None
        )
        .unwrap(),
        1
    );
}

#[test]
fn e2e_some_and_none() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }"),
            None
        )
        .unwrap(),
        42
    );
}

#[test]
fn e2e_ok_and_err() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { let r = Ok(42); match r { Ok(v) => v, Err(_) => 0 } }"),
            None
        )
        .unwrap(),
        42
    );
}

#[test]
fn e2e_prelude_some() {
    let _ = pipeline::run(&temp_g("main = () => Some(42)"), None).unwrap();
}

#[test]
fn e2e_prelude_result() {
    let _ = pipeline::run(&temp_g("main = () => Ok(100)"), None).unwrap();
}

#[test]
fn e2e_enum_match_prelude() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }"),
            None
        )
        .unwrap(),
        42
    );
}

#[test]
fn e2e_option_tag_order() {
    let src = r#"
main = () => {
    let m: Option<i64> = Some(42);
    match m {
        Some(v) => v,
        None => 0,
    }
}
"#;
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 42);
}

