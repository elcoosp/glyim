use glyim_core::interner::Interner;
use glyim_diag::GlyimDiagnostic;
use glyim_syntax::SyntaxNode;

use super::CrateHir;
use super::lower::lower_crate;

/// Build the HIR from the AST, returning both the HIR and any diagnostics collected during lowering.
pub fn lower_crate_for_pipeline(
    root: &SyntaxNode,
    interner: &mut Interner,
) -> (CrateHir, Vec<GlyimDiagnostic>) {
    let mut diags = Vec::new();
    // `lower_crate` performs the raw syntax→HIR lowering AND the `async fn` /
    // `.await` desugar, producing the future state-machine shape the
    // type-checker understands.
    let hir = lower_crate(root, interner, &mut diags);
    (hir, diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ItemKind;
    use crate::lower::lower_crate_raw;
    use glyim_frontend::parse_to_syntax;
    use glyim_span::FileId;

    /// Plan §6.1: `const fn` must lower to an `FnItem` with `is_const == true`,
    /// while a plain `fn` stays `is_const == false`.
    fn lower_first_fn_const(src: &str) -> bool {
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let mut interner = Interner::new();
        let (hir, _) = lower_crate_for_pipeline(&root, &mut interner);
        let item = hir.items.iter().next().expect("expected at least one item");
        match &item.kind {
            ItemKind::Fn(f) => f.is_const,
            _ => panic!("expected an Fn item"),
        }
    }

    /// Plan §6.1: `async fn` must lower to an `FnItem` with `is_async == true`,
    /// while a plain `fn` stays `is_async == false`. The async desugaring
    /// itself is a larger feature (Future lang item + state machine); this test
    /// locks in the keyword → `FnItem.is_async` plumbing, matching the
    /// `is_const` pattern.
    fn lower_first_fn_async(src: &str) -> bool {
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let mut interner = Interner::new();
        // Use the RAW lower (no async desugar) so we assert the keyword →
        // `FnItem.is_async` plumbing before desugaring rewrites the item into a
        // synchronous future-returning wrapper.
        let (hir, _) = {
            let mut diags = Vec::new();
            let h = lower_crate_raw(&root, &mut interner, &mut diags);
            (h, diags)
        };
        let item = hir.items.iter().next().expect("expected at least one item");
        match &item.kind {
            ItemKind::Fn(f) => f.is_async,
            _ => panic!("expected an Fn item"),
        }
    }

    #[test]
    fn const_fn_lowers_with_is_const_true() {
        assert!(lower_first_fn_const("const fn f() { let x = 1; }"));
    }

    #[test]
    fn plain_fn_lowers_with_is_const_false() {
        assert!(!lower_first_fn_const("fn f() { let x = 1; }"));
    }

    #[test]
    fn async_fn_lowers_with_is_async_true() {
        assert!(lower_first_fn_async("async fn f() { let x = 1; }"));
    }

    #[test]
    fn plain_fn_lowers_with_is_async_false() {
        assert!(!lower_first_fn_async("fn f() { let x = 1; }"));
    }

    #[test]
    fn impl_associated_type_is_captured() {
        // Plan unstub-5 P5: `type Output = i32;` inside an `impl` must be
        // captured into `ImplItem.associated_types` so projection
        // (`Self::Output` / `F::Output`) has a defining type to resolve
        // against. Previously the impl body only lowered `fn`s, so the assoc
        // type (and thus all projection) was silently dropped.
        let src = r#"
            trait MyFuture { type Output; fn poll(&mut self) -> i32; }
            struct AddOne { x: i32 }
            impl MyFuture for AddOne { type Output = i32; fn poll(&mut self) -> i32 { self.x } }
        "#;
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let mut interner = Interner::new();
        let (hir, _) = lower_crate_for_pipeline(&root, &mut interner);
        let impl_item = hir
            .items
            .iter()
            .find(|it| matches!(it.kind, ItemKind::Impl(_)))
            .expect("expected an impl item");
        match &impl_item.kind {
            ItemKind::Impl(impl_item) => {
                assert_eq!(
                    impl_item.associated_types.len(),
                    1,
                    "impl must capture exactly one associated type"
                );
                assert!(
                    impl_item.associated_types[0].default.is_some(),
                    "impl assoc type must carry its defining type"
                );
            }
            _ => panic!("expected an Impl item"),
        }
    }

    #[test]
    fn trait_associated_type_is_captured() {
        // Plan unstub-5 P5: `type Output;` inside a `trait` must be captured
        // into `TraitItem.associated_types` so `TraitDef` (and thus the
        // impl-check / projection machinery) knows the trait carries an
        // associated type. Previously `lower_trait_def` hardcoded
        // `associated_types: Vec::new()`, dropping the declaration.
        let src = r#"
            trait MyFuture { type Output; fn poll(&mut self) -> i32; }
            struct AddOne { x: i32 }
            impl MyFuture for AddOne { type Output = i32; fn poll(&mut self) -> i32 { self.x } }
        "#;
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let mut interner = Interner::new();
        let (hir, _) = lower_crate_for_pipeline(&root, &mut interner);
        let trait_item = hir
            .items
            .iter()
            .find(|it| matches!(it.kind, ItemKind::Trait(_)))
            .expect("expected a trait item");
        match &trait_item.kind {
            ItemKind::Trait(trait_item) => {
                assert_eq!(
                    trait_item.associated_types.len(),
                    1,
                    "trait must capture exactly one associated type"
                );
                assert_eq!(
                    trait_item.associated_types[0].name,
                    interner.intern("Output"),
                    "trait assoc type must be named `Output`"
                );
                assert!(
                    trait_item.associated_types[0].default.is_none(),
                    "trait assoc-type declaration must not carry a defining type (that lives on the impl)"
                );
            }
            _ => panic!("expected a Trait item"),
        }
    }
}
