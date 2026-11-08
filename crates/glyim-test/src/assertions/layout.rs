use glyim_core::primitives::TargetInfo;
use glyim_layout::LayoutComputer;
use glyim_layout::SimpleLayoutComputer;
use glyim_type::{Ty, TyCtx};

/// `assert_layout`.
pub fn assert_layout(ctx: &TyCtx, ty: Ty, expected_size: u64, expected_align: u64) {
    let computer = SimpleLayoutComputer::new(ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(ty)
        .unwrap_or_else(|e| panic!("layout computation failed: {e:?}"));
    assert_eq!(
        layout.size.0, expected_size,
        "layout size mismatch: expected {}, got {}",
        expected_size, layout.size.0
    );
    assert_eq!(
        layout.align.0, expected_align,
        "layout align mismatch: expected {}, got {}",
        expected_align, layout.align.0
    );
}
