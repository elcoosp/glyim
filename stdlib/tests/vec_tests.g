#[test]
fn test_vec_new_is_empty() {
    let v = Vec::new()
    assert(v.len() == 0)
    assert(v.is_empty())
}

#[test]
fn test_vec_push_and_get() {
    let v = Vec::new()
    v.push(10)
    v.push(20)
    v.push(30)
    assert(v.len() == 3)
    assert(v.get(0) == 10)
    assert(v.get(1) == 20)
    assert(v.get(2) == 30)
}

#[test]
fn test_vec_pop() {
    let v = Vec::new()
    v.push(10)
    v.push(20)
    let last = v.pop()
    assert(last == 20)
    assert(v.len() == 1)
    assert(v.get(0) == 10)
}

#[test]
fn test_vec_grow_beyond_initial_capacity() {
    let v = Vec::new()
    let mut i = 0
    while i < 20 {
        v.push(i)
        i = i + 1
    }
    assert(v.len() == 20)
    assert(v.get(0) == 0)
    assert(v.get(19) == 19)
}

#[test(should_panic)]
fn test_vec_get_out_of_bounds() {
    let v = Vec::new()
    v.push(1)
    v.get(5)
}

#[test(should_panic)]
fn test_vec_pop_empty() {
    let v = Vec::new()
    v.pop()
}
