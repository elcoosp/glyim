struct Point { x, y }
main = () => { let p = Point { x: 1, y: 2 }; let Point { x } = p; x }
