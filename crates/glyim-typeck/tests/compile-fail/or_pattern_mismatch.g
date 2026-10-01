enum Result<T, E> { Ok(T), Err(E) }
fn main() {
    let res = Result::Ok::<i32, &str>(10);
    match res { //~ ERROR incompatible patterns
        Result::Ok(x) | Result::Err(_) => { let _ = x; },
    }
}
