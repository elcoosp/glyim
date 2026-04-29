#[cfg(test)]
mod debug_tests {
    use glyim_parse::parse;
    use glyim_hir::lower;
    use glyim_interner::Interner;

    #[test]
    fn debug_tuple() {
        let src = "main = () => { let p = (1, 2); p._0 }";
        let out = parse(src);
        eprintln!("Tuple errors: {:?}", out.errors);
        let mut interner = Interner::new();
        let hir = lower(&out.ast, &mut interner);
        eprintln!("Tuple HIR items:");
        for item in &hir.items {
            eprintln!("  {:?}", item);
        }
    }

    #[test]
    fn debug_impl() {
        let src = "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nmain = () => { let p = Point::zero(); p.x }";
        let out = parse(src);
        eprintln!("Impl errors: {:?}", out.errors);
        let mut interner = Interner::new();
        let hir = lower(&out.ast, &mut interner);
        eprintln!("Impl HIR items:");
        for item in &hir.items {
            eprintln!("  {:?}", item);
        }
    }
}
