use crate::analysis::GlyimAnalysis;
use crate::lang::GlyimLang;
use egg::{rewrite as rw, Rewrite};

pub fn core_rewrites() -> Vec<Rewrite<GlyimLang, GlyimAnalysis>> {
    vec![
        // Identity elimination
        rw!("add-zero"; "(+ ?a 0)" => "?a"),
        rw!("zero-add"; "(+ 0 ?a)" => "?a"),
        rw!("sub-zero"; "(- ?a 0)" => "?a"),
        rw!("mul-one";  "(* ?a 1)" => "?a"),
        rw!("one-mul";  "(* 1 ?a)" => "?a"),
        rw!("div-one";  "(/ ?a 1)" => "?a"),
        rw!("and-true"; "(&& ?a true)" => "?a"),
        rw!("true-and"; "(&& true ?a)" => "?a"),
        rw!("or-false"; "(|| ?a false)" => "?a"),
        rw!("false-or"; "(|| false ?a)" => "?a"),

        // Strength reduction
        rw!("mul-by-2"; "(* ?a 2)" => "(+ ?a ?a)"),  // x*2 = x+x (simpler than <<)
        rw!("mul-by-4"; "(* ?a 4)" => "(+ (+ ?a ?a) (+ ?a ?a))"),
        rw!("div-by-2"; "(/ ?a 2)" => "(* ?a 0.5)"),

        // Double negation
        rw!("neg-neg"; "(- (- ?a))" => "?a"),
        rw!("not-not"; "(! (! ?a))" => "?a"),

        // Boolean simplifications
        rw!("not-eq";  "(! (== ?a ?b))" => "(!= ?a ?b)"),
        rw!("not-neq"; "(! (!= ?a ?b))" => "(== ?a ?b)"),
        rw!("and-false"; "(&& ?a false)" => "false"),
        rw!("or-true";   "(|| ?a true)" => "true"),

        // Commutativity
        rw!("add-comm"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rw!("mul-comm"; "(* ?a ?b)" => "(* ?b ?a)"),
        rw!("eq-comm";  "(== ?a ?b)" => "(== ?b ?a)"),
        rw!("and-comm"; "(&& ?a ?b)" => "(&& ?b ?a)"),
        rw!("or-comm";  "(|| ?a ?b)" => "(|| ?b ?a)"),

        // Associativity
        rw!("add-assoc"; "(+ (+ ?a ?b) ?c)" => "(+ ?a (+ ?b ?c))"),
        rw!("mul-assoc"; "(* (* ?a ?b) ?c)" => "(* ?a (* ?b ?c))"),
        rw!("and-assoc"; "(&& (&& ?a ?b) ?c)" => "(&& ?a (&& ?b ?c))"),
        rw!("or-assoc";  "(|| (|| ?a ?b) ?c)" => "(|| ?a (|| ?b ?c))"),
    ]
}
