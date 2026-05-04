enum Option<T> { Some(T), None }
main = () => {
    let x: Option<Option<i64>> = Option::Some(Option::Some(42));
    match x {
        Option::Some(inner) => match inner {
            Option::Some(val) => val,
            Option::None => 0,
        },
        Option::None => 0,
    }
}
