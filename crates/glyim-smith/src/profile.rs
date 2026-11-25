//! Generator profiles — the plan CO-5 "grows by feature" axis.
//!
//! Each profile corresponds to one slice of the master-plan M4.3 scope
//! line ("scalars, control flow, structs/enums/match, functions,
//! generics") and emits fixture headers tagged with the ledger features
//! the generated programs exercise, so per-feature evidence accounting
//! (plan §3.10 RG-01) can slice a campaign by profile.

/// The v1 feature slices of [`crate::generate`], in plan order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeneratorProfile {
    /// Straight-line `i32` arithmetic chains (`LANG-SYN-OPERATORS`).
    Scalars,
    /// Bounded `while` loops and `if`/`else` statement trees
    /// (`LANG-SYN-WHILE`, `LANG-SYN-IF`).
    ControlFlow,
    /// Fieldless enums with `match` classifiers plus single-field structs
    /// read through fields and trait methods (`LANG-SYN-ENUM`,
    /// `LANG-SYN-STRUCT`, `LANG-SYN-FIELD-ACCESS`).
    StructsEnumsMatch,
    /// Top-level helper functions composed into call chains
    /// (`LANG-SYN-FN-DEF`, `LANG-SYN-RETURN`).
    Functions,
    /// Generic identity/projection functions monomorphized with `i32`
    /// (`LANG-SYN-FN-DEF` at generic arity).
    Generics,
}

impl GeneratorProfile {
    /// Every v1 profile, in plan order — iteration order of the `DoD` sample.
    pub const ALL: &'static [GeneratorProfile] = &[
        GeneratorProfile::Scalars,
        GeneratorProfile::ControlFlow,
        GeneratorProfile::StructsEnumsMatch,
        GeneratorProfile::Functions,
        GeneratorProfile::Generics,
    ];

    /// Stable kebab-case name (used in diagnostics, benches, examples).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GeneratorProfile::Scalars => "scalars",
            GeneratorProfile::ControlFlow => "control-flow",
            GeneratorProfile::StructsEnumsMatch => "structs-enums-match",
            GeneratorProfile::Functions => "functions",
            GeneratorProfile::Generics => "generics",
        }
    }

    /// The ledger feature IDs the profile's programs exercise; these go
    /// verbatim into the generated `//@ feature:` fixture header.
    #[must_use]
    pub fn feature_tags(self) -> &'static str {
        match self {
            GeneratorProfile::Scalars => "TOOL-TEST-RUN-PASS, TOOL-TEST-PROP, LANG-SYN-OPERATORS",
            GeneratorProfile::ControlFlow => {
                "TOOL-TEST-RUN-PASS, TOOL-TEST-PROP, LANG-SYN-WHILE, LANG-SYN-IF"
            }
            GeneratorProfile::StructsEnumsMatch => {
                "TOOL-TEST-RUN-PASS, TOOL-TEST-PROP, LANG-SYN-ENUM, LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS"
            }
            GeneratorProfile::Functions => {
                "TOOL-TEST-RUN-PASS, TOOL-TEST-PROP, LANG-SYN-FN-DEF, LANG-SYN-RETURN"
            }
            GeneratorProfile::Generics => {
                "TOOL-TEST-RUN-PASS, TOOL-TEST-PROP, LANG-SYN-FN-DEF, LANG-GENERIC-WHERE"
            }
        }
    }
}
