enum Option { Some, None }
main = () => {
    let m = Option::Some;
    match m {
        Option::Some => 1,
        Option::None => 0,
    }
}
