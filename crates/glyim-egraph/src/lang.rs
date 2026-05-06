use egg::define_language;
use egg::Id;

define_language! {
    pub enum GlyimLang {
        // Literals
        Num(i64),
        FNum(u64),
        BoolLit(bool),
        StrLit(String),
        // Variable reference
        Var(String),
        // Binary ops
        "+"  = Add([Id; 2]),
        "-"  = Sub([Id; 2]),
        "*"  = Mul([Id; 2]),
        "/"  = Div([Id; 2]),
        "%"  = Rem([Id; 2]),
        "==" = Eq([Id; 2]),
        "!=" = Neq([Id; 2]),
        "<"  = Lt([Id; 2]),
        ">"  = Gt([Id; 2]),
        "<=" = Lte([Id; 2]),
        ">=" = Gte([Id; 2]),
        "&&" = And([Id; 2]),
        "||" = Or([Id; 2]),
        // Unary ops
        "-" = Neg(Id),
        "!" = Not(Id),
        // Control flow
        "if" = If([Id; 3]),
        "while" = While([Id; 2]),
        // Function calls
        Call(String, Vec<Id>),
        // Fallback
        Symbol(String, Vec<Id>),
    }
}
