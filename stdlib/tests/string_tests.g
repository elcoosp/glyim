#[test]
fn test_string_new_is_empty() {
    let s = String::new()
    assert(s.len() == 0)
    assert(s.is_empty())
}

#[test]
fn test_string_from_str() {
    let s = String::from("hello")
    assert(s.len() == 5)
}

#[test]
fn test_string_multiple_from() {
    let a = String::from("foo")
    let b = String::from("longer")
    assert(a.len() == 3)
    assert(b.len() == 6)
}
