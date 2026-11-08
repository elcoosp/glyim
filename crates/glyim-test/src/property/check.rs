use super::arbitrary::{Generator, sentinel_invariant};

/// `check_ty_property`.
pub fn check_ty_property<F>(seed: u64, n_cases: usize, property: F) -> Result<(), String>
where
    F: Fn(&glyim_type::TyCtx, glyim_type::Ty) -> Result<(), String>,
{
    let mut ctx_mut = crate::test_ty_ctx();
    let mut generator = Generator::new(seed);
    let types: Vec<glyim_type::Ty> = (0..n_cases)
        .map(|_| generator.generate_ty(&mut ctx_mut, 0))
        .collect();
    let ctx = ctx_mut.freeze();
    sentinel_invariant(&ctx);
    for (i, ty) in types.iter().enumerate() {
        if let Err(msg) = property(&ctx, *ty) {
            return Err(format!(
                "case {} failed: {} (ty_kind: {:?})",
                i,
                msg,
                ctx.ty_kind(*ty)
            ));
        }
    }
    Ok(())
}
