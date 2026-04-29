use glyim_parse::parse;
use glyim_hir::lower;
use glyim_interner::Interner;

fn main() {
    // Test tuple
    let src1 = "main = () => { let p = (1, 2); p._0 }";
    let out1 = parse(src1);
    let hir1 = lower(&out1.ast, &mut Interner::new());
    println!("=== TUPLE HIR ===");
    println!("{:#?}", hir1.items);

    // Test impl method
    let src2 = "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nmain = () => { let p = Point::zero(); p.x }";
    let out2 = parse(src2);
    println!("\n=== IMPL PARSE ERRORS ===");
    println!("{:?}", out2.errors);
    let hir2 = lower(&out2.ast, &mut Interner::new());
    println!("\n=== IMPL HIR ===");
    for item in &hir2.items {
        println!("{:#?}", item);
    }

    // Test generic edge
    let src3 = "struct Edge<T> { from: T, to: T }\nimpl<T> Edge<T> {\n    fn new(from: T, to: T) -> Edge<T> { Edge { from, to } }\n}\nfn main() -> i64 {\n    let e: Edge<i64> = Edge::new(0, 100)\n    let (from, to) = (e.from, e.to)\n    from - to\n}";
    let out3 = parse(src3);
    println!("\n=== EDGE PARSE ERRORS ===");
    println!("{:?}", out3.errors);
    let hir3 = lower(&out3.ast, &mut Interner::new());
    println!("\n=== EDGE HIR ===");
    for item in &hir3.items {
        println!("{:#?}", item);
    }
}
