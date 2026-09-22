# Glyim Compiler De-Stub Plan

**Audience:** an autonomous coding agent with `cargo build`/`cargo test` access to this
repo. No prior context is assumed beyond what's in this document.

**Ground rules for the agent**

1. Work **one phase at a time, in order**. Phases are ordered by dependency — later
   phases assume earlier ones compile and pass tests. Do not skip ahead.
2. After *every* file edit: `cargo check -p <crate>` for the crate you touched, then
   `cargo test -p <crate>`. After finishing a phase: `cargo build --workspace && cargo
   test --workspace`. Do not move to the next phase with a red build.
3. Never introduce a new `TODO`, `unimplemented!()`, `todo!()`, or a comment saying
   "for now" / "simplified" / "not yet supported". If a task is genuinely out of scope
   for a phase, it must be tracked as an explicit ticket in `KNOWN_GAPS.md` (create this
   file in the repo root), not silently stubbed.
4. Every new code path needs at least one test: a positive case, and where relevant a
   negative/diagnostic case. Put compiler-level tests in the owning crate's
   `tests/` dir using the existing `glyim-test` harness conventions already used in
   that crate.
5. When this document gives a concrete diff, apply it as written, then adapt to
   whatever has drifted (field names, imports) — but do not change the *shape* of the
   fix without a reason written in the commit message.
6. Grep before you assume. Multiple items below touch the same underlying
   infrastructure (`Substitution`, `PathSegment::generic_args`, `TyCtx`) — search for
   all call sites with `rg` before declaring an item done.

---

## Phase 0 — Baseline

```bash
cargo build --workspace 2>&1 | tee /tmp/baseline_build.log
cargo test --workspace 2>&1 | tee /tmp/baseline_test.log
rg -n "todo!\(\)|unimplemented!\(\)|TODO|FIXME|for now|not yet" --type rust -g '!target' | tee /tmp/stub_inventory.txt
```

Commit these logs somewhere (e.g. `docs/baseline/`) so regressions are detectable.
`/tmp/stub_inventory.txt` is your master checklist — cross-reference it against the
phases below. Anything it finds that isn't covered by a phase still needs a
`KNOWN_GAPS.md` entry with a reason.

---

## Phase 1 — Generics End-to-End (root-cause fix, unblocks almost everything else)

### Why this is Phase 1

Items **#5, #13, #6, #24, #1** from the report are *symptoms of a single root cause*:
`PathSegment::generic_args` is always `None` because nothing in the parser or the HIR
lowerer ever populates it. Downstream, `glyim-typeck/src/tyconv.rs::resolve_qualified_path`
**already has fully correct logic** to build a `Substitution` from
`path.segments.last().generic_args` — it just never receives any. Meanwhile
`resolve_name_to_adt_ty` (the single-segment path, e.g. bare `Vec<i32>`) drops generic
args unconditionally even when they exist. Fix the pipeline once, in order, and
`Vec<i32>`, `Option<u8>`, user generic structs, etc. all start working together.

### 1.1 Frontend: parse turbofish (`x::<T>()`) and confirm type-position generics are visible to the lowerer

**File:** `glyim-frontend/src/parser/expr.rs`

Currently `parse_path_expr` never looks for `::<`. Add turbofish support right after
parsing the path segments, before returning from `parse_path_expr`:

```rust
pub(crate) fn parse_path_expr(&mut self) {
    self.start_node(SyntaxKind::PathExpr);
    self.parse_path();
    // Turbofish: `ident::<T, U>`. Only fires on `::` immediately followed by `<`,
    // so plain `a::b` paths are unaffected.
    if self.current_kind() == SyntaxKind::ColonColon && self.peek_kind() == Some(SyntaxKind::Lt) {
        self.bump(); // ::
        self.parse_type_arg_list();
    }
    self.finish_node();
    self.last_was_path = true;
}
```

`parse_type_arg_list` already exists in `glyim-frontend/src/parser/item.rs` and handles
the `>>`-splitting logic — reuse it verbatim, do not duplicate it.

**Important:** `parse_type_arg_list` currently emits the parsed type nodes as direct
children of whatever node is open (no wrapper node), which is fine for `PathType`
(single type node per path already) but means, for both `PathType` and now `PathExpr`,
the generic-arg type nodes are *siblings* of the `Ident`/`UsePath` tokens rather than
grouped. That is workable (see 1.2) but fragile if a path ever has more than one
segment with its own generic args (e.g. `a::<T>::b::<U>`, not valid in this language
today per the parser structure, so it's fine) — leave a comment explaining this
assumption:

```rust
// NOTE: generic args always belong to the *last* path segment in this grammar
// (no per-segment turbofish like `Foo::<T>::Bar::<U>`). lower_path_expr /
// lower_path_from_type below rely on this.
```

### 1.2 HIR lowering: actually populate `PathSegment::generic_args`

**File:** `glyim-hir/src/lower/lower_expr.rs`, function `lower_path_expr`

Replace the body with logic that also collects trailing type-arg nodes and attaches
them to the *last* segment:

```rust
fn lower_path_expr(node: &SyntaxNode, interner: &mut Interner, body: &mut Body) -> Option<ExprId> {
    let mut segments = Vec::new();
    let mut generic_arg_tys: Vec<TypeRef> = Vec::new();

    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Ident => {
                segments.push(PathSegment {
                    name: interner.intern(t.text()),
                    generic_args: None,
                });
            }
            glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::UsePath => {
                for t in n.children_with_tokens() {
                    if let glyim_syntax::SyntaxElement::Token(tt) = t
                        && tt.kind() == SyntaxKind::Ident
                    {
                        segments.push(PathSegment {
                            name: interner.intern(tt.text()),
                            generic_args: None,
                        });
                    }
                }
            }
            // Turbofish type args are lowered with the same lower_type_ref used
            // by ordinary type positions, so `Vec::<i32>::new()` and `Vec<i32>`
            // produce identical TypeRef::Path shapes.
            glyim_syntax::SyntaxElement::Node(n) if super::is_type_node(&n) => {
                if let Some(ty) = super::lower_type::lower_type_ref(&n, interner) {
                    generic_arg_tys.push(ty);
                }
            }
            _ => {}
        }
    }
    if segments.is_empty() {
        return None;
    }
    if !generic_arg_tys.is_empty()
        && let Some(last) = segments.last_mut()
    {
        last.generic_args = Some(generic_arg_tys);
    }
    let path = HirPath { segments, kind: PathKind::Plain };
    let expr = Expr::Path(path);
    let eid = body.alloc_expr(expr, node_span(node));
    Some(eid)
}
```

Check the exact visibility of `lower_type_ref` (it's `pub(crate)` in `lower_type.rs`) and
of `is_type_node` (`pub(crate)` in `lower/mod.rs`) — both are already `pub(crate)` in the
same crate, so no visibility changes needed, just correct `use`/module paths.

**File:** `glyim-hir/src/lower/lower_type.rs`, function `lower_path_from_type`

Same fix, but note `lower_type_ref` is the function *calling* `lower_path_from_type`, so
avoid infinite mutual recursion by inlining the per-child dispatch instead of calling
`lower_type_ref` directly on `PathType` children (there won't be nested `PathType`
directly as a generic arg's *own* representation issue — a generic arg like `Vec<Box<T>>`
is itself a `PathType` node, and calling `lower_type_ref` on it is correct and
terminates because it's a different, smaller subtree):

```rust
pub(crate) fn lower_path_from_type(node: &SyntaxNode, interner: &mut Interner) -> Option<HirPath> {
    let mut segments = Vec::new();
    let mut generic_arg_tys: Vec<TypeRef> = Vec::new();

    for el in node.children_with_tokens() {
        match el {
            glyim_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Ident => {
                segments.push(PathSegment { name: interner.intern(t.text()), generic_args: None });
            }
            glyim_syntax::SyntaxElement::Node(n) if n.kind() == SyntaxKind::UsePath => {
                for t in n.children_with_tokens() {
                    if let glyim_syntax::SyntaxElement::Token(tt) = t
                        && tt.kind() == SyntaxKind::Ident
                    {
                        segments.push(PathSegment { name: interner.intern(tt.text()), generic_args: None });
                    }
                }
            }
            glyim_syntax::SyntaxElement::Node(n) if is_type_node(&n) => {
                if let Some(ty) = lower_type_ref(&n, interner) {
                    generic_arg_tys.push(ty);
                }
            }
            _ => {}
        }
    }
    if segments.is_empty() {
        return None;
    }
    if !generic_arg_tys.is_empty()
        && let Some(last) = segments.last_mut()
    {
        last.generic_args = Some(generic_arg_tys);
    }
    Some(HirPath { segments, kind: PathKind::Plain })
}
```

**Test** (add to `glyim-hir/tests/` or wherever existing lowering tests live — grep for
an existing `lower_path` test to match conventions):

```rust
#[test]
fn generic_args_are_preserved_in_type_path() {
    let src = "struct S { v: Vec<i32> }";
    let hir = lower_test_crate(src); // use whatever the existing harness helper is called
    let field_ty = /* find field `v`'s TypeRef */;
    match field_ty {
        TypeRef::Path(p) => {
            let args = p.segments.last().unwrap().generic_args.as_ref().expect("generic args present");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected TypeRef::Path, got {other:?}"),
    }
}

#[test]
fn turbofish_args_are_preserved_in_expr_path() {
    let src = "fn f() { let x = Vec::<i32>::new(); }";
    // assert the lowered Expr::Path for `Vec::<i32>::new` has generic_args on segment `Vec`... 
    // (adjust to whatever the actual lowered segment structure is for method-style calls
    //  in this codebase — check how CallExpr wraps PathExpr first).
}
```

> Before writing the turbofish test, check how `CallExpr` around a `PathExpr` is
> represented (look at `lower_call_expr` in `lower_expr.rs`) — the turbofish belongs to
> the callee path, not the call itself, so make sure the assertion targets the right
> node.

### 1.3 typeck: stop dropping generic args on single-segment ADT paths

**File:** `glyim-typeck/src/tyconv.rs`, function `resolve_name_to_adt_ty`

This is currently:

```rust
fn resolve_name_to_adt_ty(
    ctx: &mut TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    name: Name,
) -> Option<Ty> {
    let def_id = resolve_name_to_def_id(def_map, name)?;
    let adt_id = AdtId::from_raw(def_id.local_id.to_raw());
    let substs = ctx.intern_substitution(vec![]);
    Some(ctx.mk_ty(TyKind::Adt(adt_id, substs)))
}
```

It's only ever called from `resolve_path_type` with `path.as_name()` — i.e. it doesn't
even receive the `Path`, only the bare `Name`, so it structurally *cannot* see generic
args. Change the call site and signature to pass the whole path/segment:

```rust
// in resolve_path_type, replace:
//   if let Some(name) = path.as_name()
//       && let Some(ty) = resolve_name_to_adt_ty(ctx, def_map, name)
//   { return ty; }
// with:
if path.segments.len() == 1
    && let Some(ty) = resolve_name_to_adt_ty(
        ctx, infer, def_map, diagnostics, &path.segments[0], param_map, span,
    )
{
    return ty;
}
```

```rust
fn resolve_name_to_adt_ty(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    seg: &glyim_hir::PathSegment,
    param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Option<Ty> {
    let def_id = resolve_name_to_def_id(def_map, seg.name)?;
    let adt_id = AdtId::from_raw(def_id.local_id.to_raw());
    let arity = ctx.adt_generic_arity(adt_id); // see 1.4 below — add this if missing
    let substs = build_substitution_from_generic_args(
        ctx, infer, def_map, diagnostics, seg.generic_args.as_deref(), arity, param_map, span,
    );
    Some(ctx.mk_ty(TyKind::Adt(adt_id, substs)))
}
```

Factor the substitution-building logic (currently duplicated inline inside
`resolve_qualified_path`) into a shared helper both call sites use:

```rust
/// Build a `Substitution` for an ADT reference from optional syntactic generic args,
/// checking arity and reporting a diagnostic on mismatch. When `args` is `None` and
/// `expected_arity > 0`, each missing arg becomes a fresh inference variable (so
/// `let v: Vec<_> = Vec::new();`-style partial inference keeps working) — full
/// omission (`Vec::new()` with no turbofish and no expected type) still resolves via
/// the normal inference table unification, this only prevents a hard error at the
/// path-resolution step.
fn build_substitution_from_generic_args(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    args: Option<&[TypeRef]>,
    expected_arity: usize,
    param_map: &HashMap<Name, Ty>,
    span: Span,
) -> Substitution {
    let mut arg_tys = Vec::new();
    match args {
        Some(args) => {
            if args.len() != expected_arity {
                diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    format!(
                        "wrong number of generic arguments: expected {expected_arity}, found {}",
                        args.len()
                    ),
                ));
            }
            for arg in args {
                let resolved = resolve_type_ref(ctx, infer, def_map, diagnostics, arg, param_map, span);
                arg_tys.push(GenericArg::Ty(resolved));
            }
            // pad/truncate to expected_arity with error types so codegen never
            // sees a mismatched substitution length
            while arg_tys.len() < expected_arity {
                arg_tys.push(GenericArg::Ty(Ty::ERROR));
            }
            arg_tys.truncate(expected_arity);
        }
        None => {
            for _ in 0..expected_arity {
                arg_tys.push(GenericArg::Ty(infer.new_ty_var(ctx)));
            }
        }
    }
    ctx.intern_substitution(arg_tys)
}
```

Now refactor `resolve_qualified_path` to call the same helper instead of its inline
loop (delete the duplicated code), and check the resulting arity against
`ctx.adt_generic_arity(adt_id)` there too.

### 1.4 Add ADT generic arity lookup (needed by 1.3, and by monomorphization correctness generally)

**File:** `glyim-type/src/adt_def.rs`

Check `AdtDef`'s current fields — it should already track `generic_params` for layout
of `Adt(AdtId, Substitution)` variance, but confirm. If `AdtDef` doesn't carry a
`generic_param_count: u32` (or a `Vec<GenericParamDef>`), add it and populate it during
HIR→AdtDef lowering (find where `AdtDef`s get built — likely in `glyim-typeck` or a
`glyim-pipeline` "register items" pass; grep `AdtDef {` construction sites). Then:

```rust
// glyim-type/src/ty_ctx.rs or ty_ctx_mut.rs
pub fn adt_generic_arity(&self, adt_id: AdtId) -> usize {
    self.adt_defs.get(&adt_id).map(|d| d.generic_params.len()).unwrap_or(0)
}
```

If `struct_field_map`/HIR item lowering for `StructDef`/`EnumDef` currently drops the
struct's own `<T, U>` type-param list (very likely, given the pattern of this bug), fix
that lowering too — grep `lower_struct_def` / `lower_enum_def` in
`glyim-hir/src/lower/lower_item.rs` for a `generic_params: Vec::new()` placeholder
(the same pattern already seen in `TraitItem`/`ImplItem` lowering) and wire it up to the
parser's `TypeParamList`/`TypeParam` nodes the same way `parse_type_param_list` already
emits them (parser side is already correct per the report — only lowering is missing).

### 1.5 Const generics (report #24) and const-generic array lengths (report #1, #6)

These three items are the same root cause at the *value* level (as opposed to the
*type* level fixed above): `ConstRef`/`ConstKind::Param` needs to flow from a `const N:
usize` generic parameter, through substitution, to layout and MIR.

**Step A — HIR: parse and lower `const N: TYPE` generic params.**

Check `glyim-frontend/src/parser/item.rs::parse_type_param_list` — confirm it already
accepts `const IDENT : TYPE` inside `<...>` (the `KwConst` keyword exists in
`SyntaxKind`). If not, add:

```rust
// inside the loop that parses each TypeParam
if self.current_kind() == SyntaxKind::KwConst {
    self.start_node(SyntaxKind::TypeParam);
    self.bump(); // const
    self.bump_expected(SyntaxKind::Ident);
    self.expect(SyntaxKind::Colon);
    self.parse_type();
    if self.current_kind() == SyntaxKind::Eq {
        self.bump();
        self.parse_expr(); // default value
    }
    self.finish_node();
    // ...comma handling same as type params
    continue;
}
```

In HIR (`glyim-hir/src/lib.rs`), `GenericParam` should become an enum instead of (or in
addition to) a plain type-param struct:

```rust
#[derive(Clone, Debug)]
pub enum GenericParam {
    Type { name: Name, bounds: Vec<TypeRef> },
    Lifetime { name: Name },
    Const { name: Name, ty: TypeRef },
}
```

Update every match on `GenericParam` (grep `GenericParam` across `glyim-hir`,
`glyim-typeck`, `glyim-solve`) to handle the new `Const` arm — do not use `_ => {}`,
each site needs real behavior (mirrors item #7's LSP wildcard-arm anti-pattern — do not
repeat that mistake here).

**Step B — typeck: bind const generic params into scope.**

Wherever `build_param_tys` (used throughout `tyconv.rs`) builds the `HashMap<Name, Ty>`
for type params, add a parallel `HashMap<Name, glyim_type::Const>` (a
`param_const_map`) built from `GenericParam::Const` entries, each becoming
`ConstKind::Param(ParamConst { index, name })`. Thread this map alongside
`param_map` through `resolve_type_ref`/array-length resolution (see Step C) — this
touches every call site of `resolve_type_ref`, so this is a real signature change;
do it with a single `ParamScope { tys: HashMap<Name, Ty>, consts: HashMap<Name,
Const> }` struct instead of adding a second loose parameter everywhere:

```rust
#[derive(Default, Clone)]
pub struct ParamScope {
    pub tys: HashMap<Name, Ty>,
    pub consts: HashMap<Name, glyim_type::Const>,
}
```

Then do a mechanical refactor: every function taking `param_map: &HashMap<Name, Ty>`
takes `scope: &ParamScope` instead, and every `param_map.get(name)` becomes
`scope.tys.get(name)`. This is a large but pure rename — run `cargo check` repeatedly
and fix call sites until the crate compiles clean.

**Step C — array length resolution using the const param scope.**

`glyim-hir/src/lower/lower_type.rs`'s `ArrayType` lowering already produces
`ConstRef::Literal(...)` for literal lengths, and `ConstRef::Error` otherwise (grep the
`ArrayType` arm shown above — it only special-cases `LitExpr`). Add a case for a bare
identifier referring to a const generic param or a `const` item:

```rust
} else if child.kind() == SyntaxKind::PathExpr {
    // `[T; N]` where N is a const generic param or associated/free const.
    if let Some(path) = lower_path_from_expr_node(&child, interner) {
        len = Some(ConstRef::Path(path));
    }
```

(Check whether `ConstRef` already has a `Path(HirPath)` variant — if not, add one next
to `Literal`/`Error` in `glyim-hir/src/lib.rs`.)

Then in `glyim-typeck`, wherever `ConstRef` is turned into `glyim_type::Const` (grep
`ConstRef::Literal` — likely in `tyconv.rs` near array-type resolution), add:

```rust
ConstRef::Path(path) => {
    if let Some(name) = path.as_name()
        && let Some(c) = scope.consts.get(&name)
    {
        c.clone()
    } else {
        // fall back to const-eval of a free `const` item with this path
        resolve_const_item_path(ctx, def_map, diagnostics, path, span)
            .unwrap_or(glyim_type::Const { kind: ConstKind::Error, ty: Ty::ERROR })
    }
}
```

**Step D — layout (`glyim-layout/src/lib.rs::layout_array`).**

Once const params flow through as `ConstKind::Param`, `layout_array` legitimately
*cannot* compute a size for an unmonomorphized generic array — that's correct compiler
behavior, not a stub, **as long as monomorphization substitutes the param before
layout is ever queried**. So the real fix is two-fold:

1. Confirm (or build) a monomorphization pass that substitutes `ConstKind::Param` with
   the caller's concrete const argument before codegen/layout ever runs on a generic
   function/struct instantiation — check `glyim-pipeline/src/mono_cache.rs`, which
   already exists for exactly this purpose (used by drop-glue generation, item #20).
   `layout_of` must always be called with fully substituted types post-monomorphization.
2. Make `layout_array`'s failure path a real, actionable internal-compiler-error
   instead of a silent `LayoutError::UnknownType`, so a monomorphization bug surfaces
   immediately instead of miscompiling:

```rust
_ => {
    return Err(LayoutError::UnresolvedConstGeneric {
        ty: outer_ty,
        const_kind: format!("{:?}", count.kind),
    });
}
```

Add the `UnresolvedConstGeneric` variant to `LayoutError` (grep its `enum` definition)
with a `Display` message like `"array length depends on unresolved const generic — \
this is a monomorphization bug, not a user error"`.

**Step E — `count_fields` in `glyim-borrowck/src/move_analysis.rs`.**

Once monomorphization guarantees borrow-checked MIR bodies are always post-substitution
(borrowck runs per-monomorphized-instance, same as codegen — verify this is already true
by checking the pipeline order in `glyim-pipeline`), the `ConstKind::Param` arm in
`count_fields` is dead code in practice, but keep it defensive rather than panicking:

```rust
glyim_type::ConstKind::Param(_) | glyim_type::ConstKind::Infer(_) => {
    // Borrowck always runs on monomorphized MIR (see glyim-pipeline ordering);
    // reaching here indicates the array's const generic escaped substitution.
    debug_assert!(
        false,
        "count_fields saw unsubstituted const generic in borrowck — monomorphization bug"
    );
    None
}
glyim_type::ConstKind::Bool(_)
| glyim_type::ConstKind::Char(_)
| glyim_type::ConstKind::String(_)
| glyim_type::ConstKind::FloatBits(_)
| glyim_type::ConstKind::Unit
| glyim_type::ConstKind::Error => None,
```

(Match every `ConstKind` variant explicitly — no wildcard arm, so the compiler forces
you to revisit this if `ConstKind` ever grows a new variant.)

**Tests for Phase 1** (add to `glyim-typeck/tests/` and `glyim-layout/tests/`):
- `struct Pair<T> { a: T, b: T }` — field types resolve to `T` substituted correctly at
  each use site (`Pair<i32>`, `Pair<Pair<i32>>`).
- `fn identity<T>(x: T) -> T { x }` called with explicit turbofish and with inference.
- `struct Buf<const N: usize> { data: [u8; N] }`, instantiate `Buf<16>`, assert
  `layout_of` returns size 16.
- Arity mismatch (`Vec<i32, i32>`) produces a diagnostic, not a panic.

---

## Phase 2 — Trait System Core (report #10, #11, #12, #22, #14, #15)

This phase assumes Phase 1 is done (traits/impls can be generic over `T`).

### 2.1 Associated types must actually be lowered and stored on `ImplItem`

This is a **prerequisite the report doesn't call out explicitly but is required for
#10**: `glyim-hir/src/lib.rs::ImplItem` has no field for associated type bindings at
all, and `lower_impl_def` in `glyim-hir/src/lower/lower_item.rs` only walks `FnDef`
children, silently dropping `TypeAlias` and `ConstDef` children even though the parser
(`parse_impl_def` in `glyim-frontend/src/parser/item.rs`) already emits them correctly.

**File:** `glyim-hir/src/lib.rs`

```rust
#[derive(Clone, Debug)]
pub struct ImplItem {
    pub trait_ref: Option<Path>,
    pub self_ty: TypeRef,
    pub methods: Vec<ImplMethod>,
    pub associated_types: Vec<ImplAssociatedTy>, // NEW
    pub associated_consts: Vec<ImplAssociatedConst>, // NEW
    pub generic_params: Vec<GenericParam>,
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
}

#[derive(Clone, Debug)]
pub struct ImplAssociatedTy {
    pub name: Name,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImplAssociatedConst {
    pub name: Name,
    pub ty: TypeRef,
    pub body: Option<BodyId>,
    pub span: Span,
}
```

**File:** `glyim-hir/src/lower/lower_item.rs`, in `lower_impl_def`, alongside the
existing `for method_node in node.children().filter(|c| c.kind() == SyntaxKind::FnDef)`
loop, add:

```rust
let mut associated_types = Vec::new();
for ty_node in node.children().filter(|c| c.kind() == SyntaxKind::TypeAlias) {
    let name_str = first_ident_text(&ty_node)?;
    let name = interner.intern(&name_str);
    let ty_ref_node = ty_node.children().find(|c| is_type_node(c));
    let Some(ty) = ty_ref_node.and_then(|n| lower_type_ref(&n, interner)) else {
        diags.push(GlyimDiagnostic::type_error(
            node_span(&ty_node),
            format!("associated type `{name_str}` in impl has no assigned type"),
        ));
        continue;
    };
    associated_types.push(ImplAssociatedTy { name, ty, span: node_span(&ty_node) });
}
```

(and similarly for `ConstDef` children → `associated_consts`, reusing the same
body-lowering call already used for free `const` items — factor that into a shared
`lower_const_like(node, ...)` helper if it isn't already one).

Also fix `TraitItem::associated_types` (currently hardcoded to `Vec::new()` in
`lower_item.rs`'s trait-lowering function per the earlier grep) using the same
`TypeAlias`-child-walk, this time populating `AssociatedTy { name, bounds, default }`
(bounds come from `type Item: Bound1 + Bound2;` — check whether the parser captures
bounds on impl-block/trait-block `type` declarations; if not, extend
`parse_impl_def`/`parse_trait_def`'s `KwType` arm to parse an optional `: Bound + Bound`
after the identifier, mirroring `parse_where_clause`'s bound-parsing code).

### 2.2 Projection normalization (report #10)

**File:** `glyim-type/src/ty_ctx.rs` or a new `glyim-solve/src/normalize.rs`

Add a normalization function that, given a `ProjectionTy { trait_ref, item_name }`,
looks up the concrete impl and returns its associated type binding:

```rust
/// Attempt to normalize `<Self as Trait<Args>>::item_name` to a concrete `Ty`.
/// Returns `None` if the impl isn't known yet (still generic) — callers should
/// leave the type as `TyKind::Projection` in that case, not panic.
pub fn normalize_projection(
    ctx: &TyCtx,
    impl_db: &ImplDatabase, // whatever registry already holds lowered ImplItems, keyed by trait+self-ty
    proj: &ProjectionTy,
) -> Option<Ty> {
    let self_ty = proj.trait_ref.self_ty;
    let trait_def_id = proj.trait_ref.def_id;

    for candidate in impl_db.impls_for_trait(trait_def_id) {
        // Reuse the exact unification the solver already uses to prove trait
        // obligations, so normalization and solving never disagree.
        if let Some(subst) = candidate.self_ty_matches(ctx, self_ty) {
            if let Some(assoc) = candidate.associated_types.iter().find(|a| a.name == proj.item_name) {
                return Some(subst_ty(ctx, assoc.ty, &subst));
            }
        }
    }
    None
}
```

`ImplDatabase` and `subst_ty` (a generic substitution-applier over `Ty`/`Substitution`)
may already exist in some form — grep `impls_for_trait`, `subst_ty`, `fn substitute` in
`glyim-solve` and `glyim-type` before writing new ones; the codebase already has
`Substitution`/`GenericArg` plumbing from Phase 1, this should be a thin layer on top,
**not** a new substitution engine. If no generic `Ty` substitution-applier exists yet,
write exactly one, in `glyim-type/src/substitution.rs`:

```rust
pub fn subst_ty(ctx: &mut TyCtxMut, ty: Ty, args: &[GenericArg]) -> Ty {
    match ctx.ty_kind(ty).clone() {
        TyKind::Param(p) => match args.get(p.index as usize) {
            Some(GenericArg::Ty(t)) => *t,
            _ => ty,
        },
        TyKind::Adt(id, substs) => {
            let new_args: Vec<_> = ctx.substitution_args(substs).iter()
                .map(|a| subst_generic_arg(ctx, a, args)).collect();
            ctx.mk_adt(id, ctx.intern_substitution(new_args))
        }
        TyKind::Ref(r, inner, m) => { let inner = subst_ty(ctx, inner, args); ctx.mk_ref(r, inner, m) }
        TyKind::RawPtr(inner, m) => { let inner = subst_ty(ctx, inner, args); ctx.mk_ty(TyKind::RawPtr(inner, m)) }
        TyKind::Slice(inner) => { let inner = subst_ty(ctx, inner, args); ctx.mk_ty(TyKind::Slice(inner)) }
        TyKind::Array(inner, c) => { let inner = subst_ty(ctx, inner, args); ctx.mk_ty(TyKind::Array(inner, subst_const(ctx, c, args))) }
        TyKind::Tuple(substs) => {
            let new_args: Vec<_> = ctx.substitution_args(substs).iter()
                .map(|a| subst_generic_arg(ctx, a, args)).collect();
            ctx.mk_tuple(ctx.intern_substitution(new_args))
        }
        // ... every other TyKind variant, matched explicitly, recursing where it
        // contains a Ty/Substitution/Const and returning itself unchanged otherwise
        // (Never, Unit, Bool, Int, Uint, Float, Char, String, Infer, Error).
        other => ty, // only for the genuinely atomic/non-generic-carrying kinds — enumerate them, do not use a bare `_`
    }
}
```

Wire `normalize_projection` into two places:
1. `glyim-solve/src/solver.rs::try_resolve_projection` — after proving the trait bound
   holds, also attempt normalization and, if it succeeds, unify the *caller's expected
   type* against the normalized concrete type (this is what makes `for x in
   some_iterator { ... }` know `x`'s type).
2. `glyim-typeck` wherever a `TyKind::Projection` is about to be shown to the user or
   used for further method resolution (e.g. before calling `resolve_method_call` on a
   projection type) — normalize first, and only fall back to treating it opaquely if
   normalization returns `None` (i.e. still under a generic bound like `T: Iterator`
   with no concrete impl — in that case, associated-type bound information from the
   `where` clause, not a concrete impl, should supply the type; see Phase 2.5).

Also fix `glyim-type/src/auto_trait.rs`'s `Projection` arm: instead of unconditionally
returning `AutoTraitFlags::empty()`, attempt `normalize_projection` first and recurse
into the result if found, matching the pattern already used two lines above it for
`Opaque`:

```rust
TyKind::Projection(proj) => {
    if let Some(normalized) = lookup.normalize_projection(proj) {
        return compute_auto_traits_recursive(normalized, lookup, registry, adt_reprs, cache, evaluating);
    }
    AutoTraitFlags::empty()
}
```

(`lookup.normalize_projection` means extending whatever `TypeLookup` trait already
exposes `opaque_hidden_ty` with a `normalize_projection` method backed by the same
`ImplDatabase`.)

### 2.3 Trait method resolution via dot syntax (report #11)

**File:** `glyim-typeck/src/check_expr.rs`, `resolve_method_call`

The existing `collect_for` closure already iterates *all* `ItemKind::Impl` items and
matches `method.name == method_name` regardless of whether `impl_item.trait_ref` is
`Some` — so trait impls with the method written directly in the `impl Trait for Type`
block **are already found** as long as generics unify (which Phase 1 now makes
possible). What's missing, per the report, is genuinely two things:

1. **Default trait methods** — if `impl Trait for Type { }` doesn't override a method
   that has a `default_body` on the `TraitMethod`, dot-call currently finds nothing,
   because `collect_for` only scans `impl_item.methods` (the impl's own list), never
   falls back to the trait definition's defaults.
2. **Blanket / where-bound methods** — `fn foo<T: Iterator>(t: T) { t.next() }` needs
   `.next()` resolved against the `Iterator` bound in scope, not against a concrete
   `impl`, because there is no concrete `impl` for a type parameter `T`.

Fix (1) — after collecting `found` from an impl block that has `trait_ref: Some(tr)`,
also check the corresponding `TraitItem` for unoverridden methods with defaults:

```rust
if let glyim_hir::ItemKind::Impl(impl_item) = &item.kind {
    let param_map = crate::tyconv::build_param_tys(this.ctx, &impl_item.generic_params);
    let impl_self_ty = crate::tyconv::resolve_type_ref(/* ... */);
    if this.unify(step_ty, impl_self_ty, span) {
        for method in &impl_item.methods {
            if method.name == method_name { /* existing code, unchanged */ }
        }
        // NEW: fall back to trait default methods not overridden by this impl
        if let Some(trait_ref) = &impl_item.trait_ref
            && let Some(trait_item) = this.lookup_trait_item(trait_ref)
            && !impl_item.methods.iter().any(|m| m.name == method_name)
        {
            for tmethod in &trait_item.methods {
                if tmethod.name == method_name && tmethod.default_body.is_some() {
                    let return_ty = tmethod.return_ty.as_ref().map(|rt| {
                        crate::tyconv::resolve_type_ref(this.ctx, this.infer, this.def_map,
                            this.diagnostics, rt, &param_map, span)
                    }).unwrap_or(Ty::UNIT);
                    found.push((impl_self_ty, return_ty));
                }
            }
        }
    }
}
```

(`lookup_trait_item` is a small helper you'll need to add to `TypeckCtx`/wherever `self`
is defined here — scan `self.hir.items` for `ItemKind::Trait` whose name matches
`trait_ref.as_name()`.)

Fix (2) — before falling back to the "no method found" diagnostic, add a pass over
**in-scope `where`-clause bounds and generic-parameter trait bounds** for the receiver
type when the receiver itself is `TyKind::Param`:

```rust
// After the existing steps/autoref loop finds nothing:
if candidates.is_empty()
    && let TyKind::Param(param) = self.ctx.ty_kind(recv_ty)
{
    for bound in self.current_fn_bounds_for_param(param) {
        // bound: TraitRef naming e.g. `Iterator` for this T
        if let Some(trait_item) = self.lookup_trait_item_by_def_id(bound.def_id) {
            for tmethod in &trait_item.methods {
                if tmethod.name == method_name {
                    let return_ty = tmethod.return_ty.as_ref().map(|rt| {
                        // NEW: resolve return type in a scope where `Self` = recv_ty
                        // and any associated types on the bound are looked up via
                        // the bound itself (e.g. `Self::Item`).
                        self.resolve_type_ref_with_self(rt, recv_ty, &bound, span)
                    }).unwrap_or(Ty::UNIT);
                    candidates.push((recv_ty, return_ty));
                }
            }
        }
    }
}
```

`current_fn_bounds_for_param` needs access to the enclosing function/impl's
`where_clauses` and `GenericParam` bounds — thread the current item's `where_clauses`
into the type-checking context if it isn't already available on `self`.

### 2.4 `Deref`-trait-aware auto-deref (report #12)

**File:** `glyim-type/src/ty_ctx.rs`, `deref_ty` — leave the structural version alone
(references/raw pointers must stay cheap and infallible), but **do not call it alone**
from `resolve_method_call`. Instead, in `check_expr.rs`, extend the `steps` loop to also
try the `Deref` trait when structural deref returns `None`:

```rust
let mut steps: Vec<Ty> = Vec::new();
let mut cur = Some(recv_ty);
while let Some(t) = cur {
    steps.push(t);
    cur = self.ctx.deref_ty(t).or_else(|| self.deref_via_trait(t));
    if steps.len() >= 10 {
        break; // matches Rust's own recursion guard rationale
    }
}
```

```rust
/// Look for `impl Deref for <t's Adt>` (or Deref<Target=...>) and return its
/// associated `Target` type, normalized. Returns None for non-ADTs or ADTs with
/// no Deref impl, ending the auto-deref chain — this mirrors deref_ty's contract.
fn deref_via_trait(&mut self, t: Ty) -> Option<Ty> {
    let TyKind::Adt(adt_id, substs) = self.ctx.ty_kind(t) else { return None };
    let deref_trait_id = self.lang_items.deref_trait()?; // glyim-type/src/lang_items.rs — confirm Deref is registered there; add if missing
    for item in self.hir.items.iter() {
        if let glyim_hir::ItemKind::Impl(impl_item) = &item.kind
            && let Some(tr) = &impl_item.trait_ref
            && crate::tyconv::resolve_path_to_local_def_id(self.def_map, tr)
                .is_some_and(|id| AdtId::from_raw(id.to_raw()) == deref_trait_id)
        {
            let param_map = crate::tyconv::build_param_tys(self.ctx, &impl_item.generic_params);
            let impl_self_ty = crate::tyconv::resolve_type_ref(self.ctx, self.infer,
                self.def_map, self.diagnostics, &impl_item.self_ty, &param_map, self.span);
            if self.unify(t, impl_self_ty, self.span) {
                let target = impl_item.associated_types.iter().find(|a| a.name == self.target_name);
                return target.map(|a| crate::tyconv::resolve_type_ref(self.ctx, self.infer,
                    self.def_map, self.diagnostics, &a.ty, &param_map, self.span));
            }
        }
    }
    None
}
```

Check `glyim-type/src/lang_items.rs` (105 lines already exist) — it likely already has
a pattern for registering "special" traits (e.g. for operator overloading `ops.g`,
which is already in the standard library per `glyim-lang-core/lib/ops.g`). Follow
that exact pattern for `Deref`, `DerefMut`, `Iterator`, `From`/`Into` if not already
present, rather than inventing a new registration mechanism.

Field-access auto-deref (`x.field` for `x: Box<S>`) needs the identical treatment —
find wherever field-projection type-checking happens (`check_expr.rs`, look for
`Expr::Field`) and apply the same `deref_ty(...).or_else(|| self.deref_via_trait(...))`
loop there instead of only the raw `deref_ty`.

### 2.5 Object safety (report #22)

**File:** `glyim-typeck/src/tyconv.rs`, in `resolve_type_ref`'s `dyn Trait` handling,
which currently calls into `glyim-type/src/object_safety.rs` (172 lines already exist —
read the whole file before editing, it likely already has most of the right shape).
Extend the check to cover, explicitly (no wildcard `_ => true`/`_ => false`):

- **Supertraits**: a trait is object-safe only if *all* its supertraits are
  object-safe too. Recurse through `TraitItem`'s supertrait list (if `TraitItem`
  doesn't currently store supertraits, check the parser/HIR — `parse_trait_def` should
  already parse `trait Foo: Bar + Baz` bounds after the trait name; if HIR drops them,
  fix the lowering the same way associated types were fixed in 2.1).
- **Associated type constraints**: for `dyn Trait<AssocType = X>` syntax if supported,
  or bare `dyn Trait` when `Trait` has associated types with no default — Rust's rule
  is that a trait with *unconstrained* associated types can still be object-safe as
  `dyn Trait` as long as no method signature *itself* needs to name that associated
  type outside of `Self`; keep the check conservative (reject dispatchable use of the
  unconstrained assoc type in method signatures) rather than silently allowing UB.
- **Generic methods**: reject (already correct behavior noted in the report as the one
  thing it does check) — keep this, just make sure it recurses into supertrait methods
  too, not just the trait's own declared methods.

```rust
pub fn is_object_safe(ctx: &TyCtx, trait_def: &TraitDefInfo) -> ObjectSafetyResult {
    for supertrait in &trait_def.supertraits {
        if let ObjectSafetyResult::Unsafe(reason) = is_object_safe(ctx, supertrait) {
            return ObjectSafetyResult::Unsafe(format!("supertrait not object safe: {reason}"));
        }
    }
    for method in all_methods_including_supertraits(trait_def) {
        if !method.generic_params.is_empty() {
            return ObjectSafetyResult::Unsafe(format!("method `{}` has generic parameters", method.name));
        }
        if method.self_kind == MethodSelfKind::ByValue && !method_has_where_self_sized(method) {
            // by-value self on a trait object requires `where Self: Sized` (which
            // then makes that *method* uncallable through the vtable but doesn't
            // break the trait itself) — check existing handling here, don't regress it.
        }
        if references_self_outside_receiver(ctx, method) {
            return ObjectSafetyResult::Unsafe(format!("method `{}` references Self", method.name));
        }
    }
    ObjectSafetyResult::Safe
}
```

Match whatever error-reporting convention (`ObjectSafetyResult` vs `bool` vs
`Result<(), String>`) the existing `object_safety.rs` already uses — read it first,
don't introduce a second convention.

### 2.6 Coherence: substitution-aware overlap + full orphan rules (report #14)

**File:** `glyim-typeck/src/coherence.rs`

`structural_tys_match` (seen above) already exists and does real work — the gap is
that it's the *only* check (no fundamental-type / orphan crate-boundary nuance beyond
"is the type or trait local"). Two concrete upgrades:

1. **Overlap must consider `where`-clause bounds**, not just structural shape: two
   impls `impl<T: Foo> Trait for T` and `impl<T: Bar> Trait for T` only actually
   overlap for `T: Foo + Bar`, which is a real coherence violation (Rust conservatively
   flags this as overlapping even without a witness type, since a future type could
   implement both). If the current implementation treats *any* two blanket impls over
   type parameters as always overlapping (which `structural_tys_match`'s
   `a_is_param && b_is_param => same name` branch suggests it might not even get that
   far), align it with Rust's actual (conservative) rule: two blanket impls over
   unconstrained-by-mutually-exclusive-bound type parameters are always reported as
   overlapping, unless the trait is explicitly marked with a "fundamental" disjointness
   marker (rustc has none in stable except via specialization, which this language may
   not have — if there's no specialization feature in this compiler, keep the rule
   simple: blanket impls over the same trait always overlap, full stop, and require the
   user to disambiguate).
2. **Orphan rule** needs the actual upstream/local crate distinction it currently
   probably approximates as "defined in this crate's `def_map`" — verify against
   `glyim-def-map`'s handling of external crates (grep for `extern_prelude` or
   equivalent) and implement the real rule: an impl `impl Trait for Type` is allowed
   only if `Trait` is local, **or** `Type`'s outermost type constructor is local (with
   the "uncovered type parameter" nuance for the fundamental-types allowance, e.g.
   `impl<T> Trait for Box<T>` being disallowed even though `Box` might be treated
   specially in real Rust via `#[fundamental]` — if this language has no such
   attribute, simplify to: outermost constructor must be locally-defined).

```rust
fn check_orphan_rule(&self, header: &ResolvedImplHeader) -> Result<(), GlyimDiagnostic> {
    if header.trait_def_id.map(|id| self.is_local_trait(id)).unwrap_or(false) {
        return Ok(());
    }
    match self.outermost_local_constructor(header.self_ty) {
        Some(_) => Ok(()),
        None => Err(GlyimDiagnostic::error_with_code(
            header.span,
            "E0117",
            format!(
                "only traits defined in the current crate can be implemented for \
                 types defined outside of the current crate"
            ),
        )),
    }
}
```

Add tests: two overlapping blanket impls → error; orphan impl of a foreign trait for a
foreign type → error; orphan impl of a foreign trait for a local type → OK; local trait
for a foreign type → OK.

### 2.7 HRTB coercion completeness (report #15)

**File:** `glyim-solve/src/hrtb.rs`

Extend `Predicate::Coerce` handling under a `for<'a>` binder to call the *same*
`can_coerce` used elsewhere (already shown above in `solver.rs`) rather than
`ty_struct_eq` alone, and specifically add the two cases the report calls out:

```rust
// function item / fn pointer -> closure trait objects, and concrete closures -> Fn/FnMut/FnOnce trait objects
(TyKind::Closure(closure_id, substs), TyKind::Dynamic(preds, region)) => {
    closure_implements_fn_traits(ctx, *closure_id, *substs, preds)
}
(TyKind::FnDef(fn_id, substs), TyKind::FnPtr(target_sig)) => {
    fn_def_sig_matches(ctx, *fn_id, *substs, target_sig)
}
(TyKind::FnPtr(sig), TyKind::Dynamic(preds, region)) => {
    fn_ptr_implements_fn_traits(ctx, sig, preds)
}
```

Delegate to `can_coerce` for everything already handled there (refs, raw ptrs,
array-to-slice) instead of re-implementing it inside `hrtb.rs` — import and call it,
don't duplicate.

**Tests for Phase 2:**
- `impl Iterator for Counter { type Item = u32; fn next(&mut self) -> Option<u32> {...} }`
  then `for x in Counter::new() { ... }` type-checks with `x: u32`.
- Default trait method called via dot-syntax on a type whose impl doesn't override it.
- `fn sum_it<T: Iterator<Item = i32>>(t: T) -> i32 { let mut s = 0; for x in t { s += x; } s }`
- `Box<S>` auto-derefs to call `S`'s inherent methods and access `S`'s fields.
- `dyn Trait` rejected with a clear diagnostic when `Trait` has a generic method.
- Two overlapping blanket impls rejected; orphan-rule violation rejected.

---

## Phase 3 — `where` Clauses in Generic Contexts (report #25)

**File:** `glyim-typeck/src/check_body.rs`

Extend whatever loop currently only handles simple `T: Trait` bounds to also record:

- **Associated type bounds**: `where T::Item: Clone` — these don't constrain `T`
  itself but add an obligation on the *normalized* projection `<T as Iterator>::Item`.
  Store them as `Predicate::Trait` over a `TraitRef` whose `self_ty` is
  `TyKind::Projection(...)`, which Phase 2.2's normalization-aware solver can already
  discharge once it's fed a real projection.
- **Higher-ranked bounds**: `where for<'a> F: Fn(&'a str) -> bool` — these need a
  `Binder<Predicate>` (the `Binder` type already exists per `glyim-type/src/binder.rs`)
  instead of a bare `Predicate`. If `where_clause.rs`'s `WhereClause` HIR type doesn't
  carry a `for<'a>` binder list, add one (parser support: check whether
  `parse_where_clause` already accepts a leading `for<'a>` before a bound — if not, add
  it mirroring `KwFn`'s existing lifetime/param-list parsing elsewhere in the parser).

```rust
// glyim-hir/src/where_clause.rs
#[derive(Clone, Debug)]
pub struct WherePredicate {
    pub bound_vars: Vec<Name>, // `for<'a, 'b>` — empty for non-HRTB bounds
    pub subject: WhereSubject, // NEW: enum { Param(Name), Projection(Path) }
    pub bound: TypeRef,
}

#[derive(Clone, Debug)]
pub enum WhereSubject {
    Param(Name),
    Projection(Path), // e.g. `T::Item`
}
```

In `check_body.rs`, when registering obligations for the current function/impl, convert
each `WherePredicate` into a `glyim_type::Predicate` (wrapped in `Binder` when
`bound_vars` is non-empty) and push it into the fulfillment context (`glyim-solve/src/
fulfill.rs` — check its existing API for registering obligations, reuse it rather than
adding a parallel mechanism).

**Test:** a function with `where T: Iterator, T::Item: std::fmt::Debug` compiles when
called with a concrete iterator whose item implements the debug-equivalent trait, and
fails with a clear diagnostic when it doesn't.

---

## Phase 4 — `extern` ABI / FFI (report #21)

### 4.1 Frontend + HIR: parse and preserve `extern "ABI"`

Check `glyim-frontend/src/parser/item.rs` for `KwExtern` handling on `fn` items — the
`SyntaxKind::KwExtern` and `ExternBlock` node already exist, so this may be partially
there for `extern { ... }` blocks but missing on individual `extern "C" fn`
declarations. Add (if missing):

```rust
// before parsing `fn`
let abi = if self.current_kind() == SyntaxKind::KwExtern {
    self.bump();
    if self.current_kind() == SyntaxKind::StringLit {
        let text = self.current_text().to_string(); // adapt to whatever token-text accessor exists
        self.bump();
        Some(text)
    } else {
        Some("C".to_string()) // bare `extern fn` defaults to "C"
    }
} else {
    None
};
```

Thread `abi` onto the `FnDef` node as an attribute the HIR lowerer can read (either as
a token still present under the node, or store it as an explicit field — check how
other item-level flags like `pub`/`unsafe` are currently represented and match that
pattern).

**File:** `glyim-hir/src/lib.rs` — add `pub abi: Option<Abi>` to whatever struct
represents a function signature (`FnItem`/`ImplMethod`/etc — there are likely several
call sites; grep every struct with a `params: Vec<Param>` field, they probably all need
this).

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Abi { Glyim, C, System, Fastcall /* extend as needed */ }
```

### 4.2 typeck: recognize the ABI, don't drop it

**File:** `glyim-typeck/src/tyconv.rs` — wherever function types/signatures are built
(`FnSig` construction), read the HIR `abi` field and store it on `glyim-type/src/
fn_sig.rs::FnSig` (add an `abi: Abi` field there if it's currently Glyim-only).

### 4.3 codegen: emit the right LLVM calling convention

**File:** `glyim-codegen-llvm/src/lower.rs` — wherever a call/fn-def emits an inkwell
`FunctionType`/`CallSiteValue`, set `set_call_convention`/the function's calling
convention based on `FnSig::abi`:

```rust
let cconv = match sig.abi {
    Abi::Glyim => llvm_sys::LLVMCallConv::LLVMCCallConv as u32, // whatever the existing default already is
    Abi::C => llvm_sys::LLVMCallConv::LLVMCCallConv as u32,
    Abi::System => {
        #[cfg(target_os = "windows")] { llvm_sys::LLVMCallConv::LLVMX86StdcallCallConv as u32 }
        #[cfg(not(target_os = "windows"))] { llvm_sys::LLVMCallConv::LLVMCCallConv as u32 }
    }
    Abi::Fastcall => llvm_sys::LLVMCallConv::LLVMX86FastcallCallConv as u32,
};
fn_value.set_call_conventions(cconv);
```

Also make sure `extern "C"` functions get **no name mangling** (a very common FFI
requirement) — check whatever symbol-naming function currently runs for every
`FnDef`/`FnDefId → LLVM symbol name` and add an early return for `Abi::C`/`Abi::System`
that uses the bare Glyim identifier instead of the mangled scheme.

**Tests:** an `extern "C" fn add(a: i32, b: i32) -> i32` compiles, links against a tiny
C test harness (`.c` file compiled with `cc` in the test's `build.rs` or via
`cc`-crate, matching whatever pattern `glyim-cli`'s existing linker tests use — check
`glyim-cli/src/linker.rs` for conventions already in place), and the two languages
agree on the result at runtime.

---

## Phase 5 — `async`/`.await` (report #23)

This is the largest single feature gap. Treat it as its own multi-step sub-plan.

### 5.1 Minimum viable scope

Do **not** attempt zero-cost stackless coroutines with full pinning/self-referential
struct support in one pass — that's a multi-month feature even in mature compilers.
Ship a **correct, if not maximally optimized**, state-machine desugaring first:

1. Lex/parse: `KwAsync`, `KwAwait` already exist as tokens (`SyntaxKind::KwAsync`,
   `KwAwait`). Confirm the parser actually consumes `async fn` (wrapping the body) and
   `.await` postfix on an expression — if it currently just errors or silently
   discards them (the report says "parsed but not lowered", implying the parser is
   already fine and only lowering is the gap — verify by grepping `KwAsync`/`KwAwait`
   usage in `glyim-frontend/src/parser/`).
2. Define a `Future` lang-item trait in `glyim-lang-core` (new file
   `glyim-lang-core/lib/future.g`, mirroring the existing `.g` standard-library source
   files like `iter.g`):

```glyim
// glyim-lang-core/lib/future.g
pub trait Future {
    type Output;
    fn poll(&mut self, cx: &mut Context) -> Poll<Self::Output>;
}

pub enum Poll<T> {
    Ready(T),
    Pending,
}
```

   (`Context`/`Waker` can start as minimal opaque structs — a single-threaded, no-op
   waker is enough to make `async fn` *usable* for straight-line/non-concurrent code
   first; document this limitation in `KNOWN_GAPS.md` rather than hiding it.)

3. HIR/typeck desugaring: lower `async fn foo(...) -> T { body }` into a regular `fn
   foo(...) -> impl Future<Output = T>` returning a generated state-machine struct that
   implements `Future::poll` by resuming at the last `.await` point. This is
   structurally similar to how this compiler probably already lowers `for` loops into
   `Iterator` calls (check `lower_expr.rs`'s `For` handling for the existing
   desugaring-to-trait-call pattern and follow the same approach) — a new
   `glyim-hir/src/lower/lower_async.rs` module doing:
   - Split the async body into segments at each `.await`.
   - Generate an enum `AsyncFooState { Start, AfterAwait0(..captured locals..), ...,
     Done }` and a struct wrapping it plus captured params.
   - Generate `poll`'s body as a `match` over the state that runs each segment and
     either falls through to the next state or returns `Poll::Pending` when a polled
     sub-future returns `Pending`, saving the sub-future itself into the state.

4. `.await` typeck: `expr.await` requires `expr: impl Future<Output = T>` (resolved via
   the same trait-method/projection machinery built in Phase 2) and evaluates to `T`.

### 5.2 Executor

A minimal single-threaded executor belongs in `glyim-runtime` (same crate that already
handles `process::kill` etc. — report #9) as a `poll_to_completion`-style block_on, not
a full reactor. This is enough to make `async fn` testable end-to-end without
committing to an I/O reactor design in this pass.

**Tests:** `async fn add_one(x: i32) -> i32 { x + 1 }` compiles, its generated
`poll` reaches `Poll::Ready` on first call (no real awaiting), and a `block_on` runtime
helper returns the right value. A second test with one nested `.await` on another
async fn exercises the state-machine's `Pending`/resume path using a fake future that
returns `Pending` once then `Ready`.

---

## Phase 6 — Codegen / Platform Completeness

### 6.1 SEH unwinding on Windows (report #2)

**File:** `glyim-codegen-llvm/src/lower.rs`, `emit_landingpad`

Check the `inkwell`/`llvm-sys` version pinned in `glyim-codegen-llvm/Cargo.toml`.
Funclet-based EH (`cleanuppad`/`cleanupret`/`catchswitch`/`catchpad`) needs LLVM's
funclet builder APIs, which are exposed in `llvm-sys` via
`LLVMBuildCleanupPad`/`LLVMBuildCleanupRet`/`LLVMBuildCatchSwitch`/`LLVMBuildCatchPad`/
`LLVMBuildCatchRet` — these exist in raw `llvm-sys` even when `inkwell`'s safe wrapper
doesn't cover them yet. If `inkwell` lacks a safe wrapper, drop to the raw `llvm-sys`
FFI calls directly inside this one function (`unsafe` block, scoped tightly, with a
comment explaining why raw FFI is needed here specifically) rather than upgrading the
whole crate's dependency (that's a bigger, separable migration — track it in
`KNOWN_GAPS.md` if you decide to defer the raw-FFI approach and want an inkwell
upgrade instead, but ship *something real* now).

```rust
fn emit_landingpad_seh(&mut self, /* existing params */) -> LandingPadResult {
    use llvm_sys::core::*;
    unsafe {
        // cleanuppad with no arguments (Windows x64 SEH cleanup funclet)
        let cleanup_args: [LLVMValueRef; 0] = [];
        let cleanuppad = LLVMBuildCleanupPad(
            self.builder.as_mut_ptr(), // adapt to inkwell's raw-handle accessor, e.g. .as_mut_ptr()/.into_raw() — check inkwell's actual API for extracting the underlying LLVMBuilderRef
            std::ptr::null_mut(), // parent pad, null for outermost
            cleanup_args.as_ptr() as *mut _,
            0,
            b"cleanup\0".as_ptr() as *const _,
        );
        // ... run cleanup (drop glue) actions here, same as the existing Itanium landingpad path ...
        let cleanup_bb = /* current block */;
        LLVMBuildCleanupRet(self.builder.as_mut_ptr(), cleanuppad, unwind_dest_bb);
    }
    LandingPadResult::Emitted
}
```

This needs real care around inkwell's ownership model for raw pointers — before writing
this, check whether `inkwell::builder::Builder` exposes `as_mut_ptr()` (common in
inkwell versions) and whether `inkwell::values::BasicValueEnum` can wrap an
`LLVMValueRef` you got back from a raw call (`BasicValueEnum::new`/similar
constructor) so the rest of the existing Itanium-path code (which presumably works in
terms of inkwell's safe types) can consume the funclet result uniformly. If inkwell
truly provides zero escape hatch, vendor a tiny `extern "C"` binding module
(`glyim-codegen-llvm/src/raw_seh.rs`) declaring just the four/five functions needed,
matching `llvm-sys`'s existing linkage.

Emit funclet-aware `invoke`/`call` for every callsite inside an SEH-landingpad function
too — the existing Itanium path's callsite-to-landingpad wiring needs an SEH-specific
counterpart (`funclet` operand bundle on each `call`/`invoke` inside the funclet, via
`LLVMBuildCall2` with `LLVMOperandBundle` — check whether `llvm-sys` exposes
`LLVMBuildCallWithOperandBundles`).

**Test:** a Windows CI job (or, if Windows CI isn't available, an `x86_64-pc-windows-
msvc` cross-compile-and-inspect-IR test that doesn't execute but asserts the emitted
`.ll`/`.o` contains `cleanuppad`/`cleanupret` and no `landingpad` when targeting MSVC)
that panics through 2+ stack frames and confirms destructors run (if execution is
possible via `wine`/actual Windows runner) or, at minimum, that codegen no longer
returns the internal error.

### 6.2 Debug info for enums and closures (report #3)

**File:** `glyim-codegen-llvm/src/debug.rs`, `debug_type_for_ty`

Replace the two opaque-struct branches:

```rust
TyKind::Adt(adt_id, substs) if self.ctx.adt_repr(*adt_id).is_some_and(|r| r.kind == AdtKind::Enum) => {
    let repr = self.ctx.adt_repr(*adt_id).unwrap();
    let variant_members: Vec<_> = repr.variants.iter().enumerate().map(|(i, variant)| {
        let variant_struct_di = self.dibuilder.create_struct_type(
            self.compile_unit.get_file(),
            &variant.name_str(self.ctx),
            self.compile_unit.get_file(),
            0,
            variant.layout.size.bytes(),
            variant.layout.align.bytes() as u32,
            0,
            None,
            &variant.fields.iter().map(|f| self.debug_type_for_ty(f.ty, substs)).collect::<Vec<_>>(),
            0, None, &variant.name_str(self.ctx),
        );
        variant_struct_di.as_type()
    }).collect();
    // DW_TAG_variant_part / union-of-variants, discriminant member first
    let discr_member = self.dibuilder.create_member_type(
        self.compile_unit.get_file(), "discriminant", self.compile_unit.get_file(), 0,
        repr.discriminant_layout.size.bits(), repr.discriminant_layout.align.bits() as u32, 0, 0,
        self.debug_type_for_ty(repr.discriminant_ty, substs),
    );
    let union_di = self.dibuilder.create_union_type(
        self.compile_unit.get_file(), &self.ctx.adt_name_str(*adt_id), self.compile_unit.get_file(),
        0, layout.size.bits(), layout.align.bits() as u32, 0, &variant_members, 0, &self.ctx.adt_name_str(*adt_id),
    );
    self.dibuilder.create_struct_type(
        self.compile_unit.get_file(), &self.ctx.adt_name_str(*adt_id), self.compile_unit.get_file(), 0,
        layout.size.bits(), layout.align.bits() as u32, 0, None,
        &[discr_member.as_type(), union_di.as_type()], 0, None, &self.ctx.adt_name_str(*adt_id),
    ).as_type()
}
```

(Match the exact `inkwell::debug_info::DebugInfoBuilder` method names/signatures
present in the pinned inkwell version — `create_union_type`/`create_struct_type`
argument orders vary across inkwell releases; check the version in `Cargo.toml` and
consult that exact API rather than copying this verbatim.)

For closures:

```rust
TyKind::Closure(closure_id, substs) => {
    let captures = self.ctx.closure_captures(*closure_id); // add this accessor if missing — captures should already be tracked for codegen's actual closure-struct layout, reuse that, don't recompute
    let members: Vec<_> = captures.iter().map(|c| {
        self.dibuilder.create_member_type(
            self.compile_unit.get_file(), &c.name_str(self.ctx), self.compile_unit.get_file(), 0,
            /* size/align/offset from the same layout codegen already computed for the closure struct */
            0, 0, 0, 0, self.debug_type_for_ty(c.ty, substs),
        )
    }).collect();
    self.dibuilder.create_struct_type(
        self.compile_unit.get_file(), &format!("{{closure#{}}}", closure_id.index()),
        self.compile_unit.get_file(), 0, layout.size.bits(), layout.align.bits() as u32, 0, None,
        &members, 0, None, "closure",
    ).as_type()
}
```

Whatever struct codegen already builds for a closure's capture layout (search
`glyim-codegen-llvm` for the existing closure-struct-type construction used for actual
codegen, not debug info) is the source of truth for field order/offsets — debug info
must match it exactly or debuggers will show garbage; don't recompute layout
independently.

**Test:** compile a small program with an enum with 2+ variants and a capturing
closure with `-g`, run it under `lldb`/`gdb` in batch mode (or just `llvm-dwarfdump
--verify` on the object file if an interactive debugger isn't available in CI) and
assert the DWARF contains real member/variant info, not an opaque blob.

### 6.3 `PlaceCollector`/`ReadVisitor` completeness (report #19)

**File:** `glyim-codegen-llvm/src/lower.rs`

Find the `ReadVisitor` impl and its terminator handling. The fix is exhaustive
enumeration, not cleverness: every `Terminator` variant that reads a `Place` — `Call`
(callee + args), `SwitchInt` (discriminant operand), `Assert` (condition), `Drop`
(the dropped place itself, as a read for borrowck purposes before the drop consumes
it), `Return` (the return place, if the calling convention reads it directly, e.g.
struct-return-via-sret patterns) — must call `self.visit_place`/`self.visit_operand`
on all of them. Write it as a match with **no `_ => {}` arm**; let the compiler force
you to handle new terminator kinds if they're ever added:

```rust
impl Visitor for ReadVisitor<'_> {
    fn visit_terminator(&mut self, term: &Terminator) {
        match &term.kind {
            TerminatorKind::Goto { .. } => {}
            TerminatorKind::SwitchInt { discr, .. } => self.visit_operand(discr),
            TerminatorKind::Call { func, args, destination, .. } => {
                self.visit_operand(func);
                for a in args { self.visit_operand(a); }
                // destination is a WRITE, not a read — do not visit it here;
                // confirm the write-side collector already handles it separately.
            }
            TerminatorKind::Assert { cond, .. } => self.visit_operand(cond),
            TerminatorKind::Drop { place, .. } => self.visit_place(place),
            TerminatorKind::Return => { /* handled by the write/read split already in place for the return local, if any — verify */ }
            TerminatorKind::Unreachable | TerminatorKind::Resume | TerminatorKind::Abort => {}
        }
    }
}
```

Adjust variant names to whatever `glyim-mir`'s actual `TerminatorKind` enum defines
(grep it before writing this).

### 6.4 Windows graceful process signaling (report #9)

**File:** `glyim-runtime/src/lib.rs`, `glyim_process_kill`, Windows branch

```rust
#[cfg(windows)]
fn glyim_process_kill(pid: u32, signal: Signal) -> Result<(), std::io::Error> {
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT, CTRL_C_EVENT};
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    match signal {
        Signal::Term | Signal::Int => {
            // Graceful: send a console control event. This only works for processes
            // in the same console process group (created with CREATE_NEW_PROCESS_GROUP);
            // if the target wasn't spawned that way, fall back to TerminateProcess
            // and surface that fact rather than silently hard-killing.
            let ctrl_event = if matches!(signal, Signal::Int) { CTRL_C_EVENT } else { CTRL_BREAK_EVENT };
            let ok = unsafe { GenerateConsoleCtrlEvent(ctrl_event, pid) };
            if ok != 0 {
                return Ok(());
            }
            // fall through to hard kill, but this is now an explicit, documented fallback
        }
        Signal::Kill => {}
    }
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle == 0 { return Err(std::io::Error::last_os_error()); }
        let ok = TerminateProcess(handle, 1);
        windows_sys::Win32::Foundation::CloseHandle(handle);
        if ok == 0 { return Err(std::io::Error::last_os_error()); }
    }
    Ok(())
}
```

Add `windows-sys` (or whatever Windows FFI crate the rest of `glyim-runtime` already
uses — check `Cargo.toml` first, don't introduce a second Windows FFI dependency if one
is already present) with the `Win32_System_Console` and `Win32_System_Threading`
features enabled. Note in the doc comment that `GenerateConsoleCtrlEvent` requires the
target process to share the caller's console or be in a process group created with
`CREATE_NEW_PROCESS_GROUP` — if `glyim_process_spawn` doesn't already set that flag on
Windows, add it so graceful signaling actually has a chance of working end-to-end.

**Test:** spawn a long-running child (`sleep`-equivalent test helper binary) with
`glyim_process_spawn`, send `Signal::Term`, assert it exits (via a cooperative
handler in the test helper reacting to the console event) rather than being hard-
killed; separately test `Signal::Kill` still hard-kills.

---

## Phase 7 — Execution Backends (report #4, #8, #26)

### 7.1 Bytecode VM (report #4, #26)

The report is explicit that no interpreter exists at all — the "backend" only emits
opcodes that golden tests assert against syntactically. Two honest options:

- **(A) Build a real tree-walking or switch-dispatch bytecode interpreter** in a new
  `glyim-bytecode-vm` crate, implementing every opcode the emitter
  (`glyim-codegen/src/lib.rs`) produces, OR
- **(B) Delete the bytecode backend** from the production compile path (keep it only
  behind a `--unstable-bytecode-backend` flag explicitly documented as
  test/experimentation-only, not a supported target) if a full VM is out of scope for
  this milestone.

Given the report frames the compiler as needing to reach "production-ready", and a
bytecode backend nobody can execute is actively misleading (it *looks* like a working
compilation target), the correct production-grade action is **(A)**, scoped down to the
opcode set the existing emitter actually produces (do not gold-plate a bytecode ISA
beyond what's emitted):

1. `cd glyim-codegen/src/lib.rs`, enumerate every opcode variant emitted (grep the
   `Opcode`/instruction enum this backend targets).
2. Create `glyim-bytecode-vm/src/lib.rs` with a `struct Vm { stack: Vec<Value>, frames:
   Vec<Frame>, ... }` and a `fn run(&mut self, chunk: &Chunk) -> ExecResult` that
   dispatches every opcode via a `match` (again: no wildcard arm — every opcode the
   emitter can produce must execute correctly or the build should fail to compile until
   it's handled).
3. Wire the existing golden-pattern tests (which currently only assert emitted bytes)
   to *also* execute the bytecode through the new VM and assert the runtime result
   matches the equivalent LLVM-compiled-and-run result for the same source — this
   closes the loop and proves the two backends agree, which is the real definition of
   "not a stub" for a second backend.
4. Implement `emit_place_address`'s `Index` projection properly against the new VM's
   actual memory model (the report's cited comment is literally admitting the opcode
   was speculative) — once the VM exists, write the `Index` case to match how the VM
   represents arrays/slices in its `Value`/stack-slot model, and add a test indexing
   into an array through several levels of struct nesting.
5. Bytecode optimizations (report #26) are legitimately optional for correctness —
   defer to `KNOWN_GAPS.md` as a perf item *once* correctness (`run()` producing right
   answers) is proven by the golden+execution tests above. Do not claim "production
   grade" for a slow-but-correct VM; a v1 with zero optimization passes that executes
   correctly is a legitimate, honest milestone — just say so.

### 7.2 Cross-frame unwinding in the MIR interpreter (report #8)

**File:** `glyim-mir-interp/src/lib.rs`, `run_current_function`

The interpreter needs an explicit call stack of `Frame`s (it must already have one to
support nested calls at all — check its existing `Vec<Frame>`/similar). Add unwind
propagation:

```rust
enum StepResult {
    Continue,
    Return(Value),
    Unwind(PanicPayload),
}

fn run_current_function(&mut self) -> StepResult {
    loop {
        match self.step() {
            StepResult::Unwind(payload) => {
                // Run this frame's own cleanup blocks first (existing single-frame logic)
                if let Some(cleanup_bb) = self.current_frame().cleanup_block_for(self.current_bb) {
                    self.current_frame_mut().jump_to(cleanup_bb);
                    continue; // stay in this frame, run its cleanup
                }
                // No cleanup left in this frame: pop it and propagate to the caller.
                self.frames.pop();
                if self.frames.is_empty() {
                    return StepResult::Unwind(payload); // reached the top — process aborts/exits per panic=unwind vs abort strategy
                }
                // Resume the caller at ITS unwind edge for the call we just returned from
                let caller = self.current_frame_mut();
                caller.jump_to(caller.pending_call_unwind_target());
                self.pending_unwind = Some(payload);
                continue;
            }
            StepResult::Return(v) => return StepResult::Return(v),
            StepResult::Continue => continue,
        }
    }
}
```

The key structural pieces this needs that may not exist yet:
- Every `Call` terminator's `unwind` target (MIR terminators already carry this per
  standard MIR shape — check `glyim-mir`'s `TerminatorKind::Call` fields for an
  `unwind: UnwindAction` field; if it's parsed/lowered but the interpreter ignores it,
  that's exactly this gap) must be recorded per-frame so the popped caller knows where
  to resume.
- A `pending_unwind` payload slot so the resumed caller frame's cleanup block can
  re-raise (`Resume` terminator) once its own cleanups finish, continuing the walk
  further up the stack.

**Test:** three nested function calls, innermost panics, each of the two callers has a
local with an observable `Drop` side effect (e.g. push to a shared `Vec<&str>` log);
assert the log shows both callers' drops ran, in the correct (innermost-first) order,
before the panic reaches the top and the process reports the expected panic message —
run this same source through the LLVM-compiled path too and assert both backends
produce the identical drop-order log (this doubles as a Phase 7.1 cross-backend
consistency test).

---

## Phase 8 — LSP Completeness (report #7, #27)

### 8.1 Reference graph (`walk_expr` wildcard arm)

**File:** `glyim-lsp/src/reference_graph.rs`

```rust
fn walk_expr(&mut self, expr: ExprId, body: &Body) {
    match &body.exprs[expr] {
        Expr::Missing | Expr::Literal(_) => {}
        Expr::Path(path) => self.record_path_use(path, expr),
        Expr::Block { stmts, tail } => {
            for s in stmts { self.walk_stmt(*s, body); }
            if let Some(t) = tail { self.walk_expr(*t, body); }
        }
        Expr::If { cond, then_branch, else_branch } => {
            self.walk_expr(*cond, body);
            self.walk_expr(*then_branch, body);
            if let Some(e) = else_branch { self.walk_expr(*e, body); }
        }
        Expr::While { cond, body: b } => { self.walk_expr(*cond, body); self.walk_expr(*b, body); }
        Expr::Loop { body: b } => self.walk_expr(*b, body),
        Expr::For { pat, iterable, body: b } => {
            self.walk_pat(*pat, body);
            self.walk_expr(*iterable, body);
            self.walk_expr(*b, body);
        }
        Expr::Range { start, end, .. } => {
            if let Some(s) = start { self.walk_expr(*s, body); }
            if let Some(e) = end { self.walk_expr(*e, body); }
        }
        Expr::Cast { expr: e, .. } => self.walk_expr(*e, body),
        Expr::Ref { expr: e, .. } => self.walk_expr(*e, body),
        Expr::Continue { .. } => {}
        Expr::Break { value, .. } => { if let Some(v) = value { self.walk_expr(*v, body); } }
        Expr::Return { value } => { if let Some(v) = value { self.walk_expr(*v, body); } }
        Expr::Closure { params, body: cbody } => {
            for p in params { self.walk_pat(*p, body); }
            self.walk_expr(*cbody, body);
        }
        Expr::Index { base, index } => { self.walk_expr(*base, body); self.walk_expr(*index, body); }
        Expr::Struct { fields, .. } => { for (_, v) in fields { self.walk_expr(*v, body); } }
        Expr::Array { elems } => { for e in elems { self.walk_expr(*e, body); } }
        Expr::Tuple { elems } => { for e in elems { self.walk_expr(*e, body); } }
        Expr::Let { pat, expr: e } => { self.walk_pat(*pat, body); self.walk_expr(*e, body); }
        // ... every remaining Expr variant already handled before this fix, unchanged
    }
}
```

Match field names to the actual `Expr` enum in `glyim-hir/src/lib.rs` (grep it — the
names above are inferred from context and will need adjusting to the real variant
shapes). The instruction that matters is structural: **delete the `_ => {}` arm
entirely** so the match becomes exhaustive; let `cargo check` tell you every variant
you still owe a case, and give each one a real traversal, not a no-op.

**Test:** rename a variable used only inside a `for` loop body, inside a closure
capture, and inside a struct-literal field value — assert all three usages are found
and renamed (this directly tests the three example gaps called out by the report).

### 8.2 Completion via trait impls, full hover, robust rename (report #27)

- **Completion**: once Phase 2.3's trait-aware `resolve_method_call` exists, LSP
  completion for `x.` should call the *same* candidate-collection logic (refactor
  `collect_for` out of `check_expr.rs` into a shared function both typeck and
  `glyim-lsp`'s completion provider call, rather than LSP re-implementing method
  lookup independently — check whether `glyim-lsp` already depends on `glyim-typeck`;
  if so this is a straightforward shared-function extraction).
- **Hover**: extend whatever hover-info builder exists to call into the same
  `check_body`/inference results already computed for diagnostics (the type-checked
  body should already have a `Ty` for every `ExprId` in an internal side table, if
  hover only reads "symbols" it's likely not consulting this table — wire it up) so
  hover works for arbitrary sub-expressions, not just named symbols.
- **Rename**: once 8.1 makes `reference_graph` exhaustive, delete the text-based
  fallback path entirely (a full reference graph should never need it) — keep it only
  as a `debug_assert!`-guarded consistency check in tests (run text-based rename and
  graph-based rename on the same fixture and assert they agree) rather than as
  production fallback logic, since two disagreeing implementations is itself a bug
  surface.

---

## Phase 9 — Macro System (report #17, #28)

### 9.1 `concat_idents!` hygiene (report #17)

**File:** `glyim-meta/src/expander/mod.rs`

The fix is to compute the synthesized identifier's syntax context from its
constituent argument contexts rather than always using the root context. If this
compiler's hygiene model is a `SyntaxContext`/mark-based system (standard for
`macro_rules!`-style hygiene — check `glyim-meta` for a `SyntaxContext`/`Mark`/`HygieneId`
type), the correct behavior mirrors what real Rust settled on: `concat_idents!`
produces an identifier hygienic with respect to the **macro expansion site**, not any
one argument, because the result names something new that didn't syntactically exist
in any input. So "root syntax context" may actually be defensible — but *only* if
"root" means "the macro's own expansion-site context", not literally the crate-root/no-
context. Verify which one the current code does:

```rust
// current (report-quoted) code roughly does:
let combined_name = args.iter().map(|a| a.text()).collect::<String>();
let synthesized = interner.intern(&combined_name);
// ... emitted with SyntaxContext::ROOT (or similar) unconditionally
```

Fix:

```rust
let combined_name = args.iter().map(|a| a.text()).collect::<String>();
let synthesized = interner.intern(&combined_name);
let ctxt = self.current_expansion_context(); // the context of the concat_idents! call site itself, NOT any argument
let synthesized_ident = Ident { name: synthesized, ctxt };
```

Add a hygiene regression test: two separate macro invocations that each expand to a
`concat_idents!`-produced local variable with the same textual name must NOT collide
(each should resolve only within its own expansion), matching real Rust's (still
unstable, but well-specified) behavior for this macro.

### 9.2 Procedural macros (report #28)

This is a large, separable feature. Minimum production-grade scope:

1. **Function-like proc macros only, first** (`my_macro!(...)` where `my_macro` is
   defined via `#[proc_macro]` in a separate compiled crate) — this avoids needing
   derive/attribute macro's more complex item-position expansion rules for v1.
2. Proc-macro crates must compile to a **cdylib/separate compilation unit** loaded by
   the compiler driver (`glyim-cli`) at macro-expansion time — this needs a real
   two-stage build: `glyim-cli` must be able to build a proc-macro crate for the *host*
   target (even when cross-compiling the main crate) and `dlopen`/`libloading` it, then
   call its registered entry point with a `TokenStream` and get a `TokenStream` back.
3. Define the `proc_macro::TokenStream` bridge type and the ABI contract between the
   compiler and the loaded macro crate (this needs to be a stable-ish C-compatible
   boundary — do not pass raw Rust-internal HIR/AST types across the dylib boundary;
   serialize to a token stream representation, same as real Rust does).
4. Wire invocation into `glyim-meta`'s existing macro-expansion pipeline alongside
   `macro_rules!` expansion, dispatching by whether the macro name resolves to a
   declarative or procedural definition.

Given the size, split this into its own tracked epic with sub-tickets in
`KNOWN_GAPS.md` rather than a single monolithic change; land derive and attribute
macros as follow-ups once function-like proc macros round-trip correctly end to end.

**Test:** a trivial `#[proc_macro] pub fn identity(input: TokenStream) -> TokenStream {
input }` proc-macro crate, invoked from a normal Glyim crate, produces byte-identical
expansion to the input.

---

## Phase 10 — Build & Tooling Polish (report #16, #29, #30)

### 10.1 `glyip` registry feature on by default (report #16)

**File:** `glyip/Cargo.toml`

```toml
[features]
default = ["registry"]
registry = ["dep:reqwest", "dep:semver", ...] # whatever the feature already gates
```

Verify this doesn't blow up build times/binary size unacceptably for users who
genuinely don't need registry support (e.g. fully vendored/offline builds) — if it
does, keep `registry` opt-out-able but flip the *default* on, and document the
`--no-default-features` escape hatch in `glyip`'s README instead of leaving registry
support opt-in by default (opt-in-by-default is what actually breaks the common case
the report is flagging).

### 10.2 LTO / ThinLTO (report #29)

**File:** `glyim-codegen-llvm/src/passes.rs`

Add LTO as a real, tested pipeline stage, not just a flag that's silently ignored:

```rust
pub fn run_lto(module: &Module, kind: LtoKind) -> Result<(), CodegenError> {
    match kind {
        LtoKind::None => Ok(()),
        LtoKind::Thin => {
            // Requires per-module summary emission at compile time and a link-time
            // merge step in glyim-cli's linker invocation (ThinLTO needs the linker's
            // cooperation — lld/gold plugin, or LLVM's own thin-link API). Check
            // glyim-cli/src/linker.rs for the current linker driver and extend its
            // invocation to pass `-flto=thin`-equivalent flags/plugin options for
            // the selected linker (lld vs system ld vs link.exe), rather than trying
            // to hand-roll ThinLTO merging inside the compiler.
            todo_marker_for_real_implementation_not_a_stub();
        }
        LtoKind::Fat => {
            // Fat LTO: merge all module IR into one before final codegen. This CAN be
            // done entirely inside glyim-codegen-llvm using LLVMLinkModules2 across all
            // per-crate/per-CGU modules before running the optimization pipeline once
            // over the merged module.
            Ok(())
        }
    }
}
```

(Remove the placeholder call — it's shown here only to mark that ThinLTO's real
implementation is a **linker-driver integration task**, not purely a `glyim-codegen-
llvm` task; scope it explicitly as touching both crates in the actual PR, and land Fat
LTO first since it's self-contained and immediately gives most of the size/speed win.)

Add a `-C lto=off|thin|fat` CLI flag in `glyim-cli` wired to `LtoKind`.

**Test:** compile a small multi-module program with `lto=fat`, verify the resulting
binary is smaller than the non-LTO build and that an obviously cross-module-inlinable
function actually gets inlined (check the disassembly/IR for the call instruction's
absence).

### 10.3 Public API documentation (report #30)

Mechanical but real: for every crate with `#![allow(missing_docs)]`, remove the
allow, run `cargo doc --workspace 2>&1 | grep "missing documentation"`, and write a real
doc comment (not a copy of the item name) for every flagged public item — one crate at
a time, smallest crate first (`glyim-core`, `glyim-span`, `glyim-diag` are good
starting points per their line counts) so this can land incrementally without one
giant PR. Do not write doc comments that just restate the signature
(`/// Returns a Ty.` on a fn named `fn ty(&self) -> Ty` is not acceptable) — each must
explain *why*/*when*, e.g. what invariant the caller can rely on.

---

## Definition of Done (apply after every phase, and once more at the very end)

- [ ] `rg "todo!\(\)|unimplemented!\(\)"` returns zero hits in non-test code.
- [ ] `rg -i "for now|simplified|left as empty|treat as opaque|is unimplemented"`
      returns zero hits, or each hit has a corresponding `KNOWN_GAPS.md` entry with a
      justification (e.g. ThinLTO's linker-integration half, proc-macro derive/attribute
      forms) — a *plan document* is allowed to defer genuinely large, separable
      features, but every deferral must be visible, not silent.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `cargo test --workspace` is green, including every test added in each phase
      above.
- [ ] `cargo doc --workspace` produces no "missing documentation" warnings once
      Phase 10.3 lands (or has an explicit, scoped `#![allow(missing_docs)]` left only
      on crates not yet migrated, tracked in `KNOWN_GAPS.md`).
- [ ] Every item number (1–30) from the original report has either a merged fix with
      tests referenced above, or a `KNOWN_GAPS.md` entry explaining the remaining scope
      and why it's tracked separately (this applies realistically to parts of #23
      async, #28 proc macros, and #29 ThinLTO's linker half — everything else in this
      plan is expected to reach full completion, not just a tracked gap).
