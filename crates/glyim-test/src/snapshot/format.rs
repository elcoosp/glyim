use glyim_core::interner::Interner;
use glyim_core::primitives::Mutability;
use glyim_mir::{LocalDecl, LocalIdx, VarDebugInfoValue};
use glyim_type::TyCtx;
use std::collections::HashMap;
use std::sync::OnceLock;

// Names interned for snapshot annotations must round-trip through a single
// interner instance (lasso's ThreadedRodeo is per-instance). The test that
// builds a body and this formatter must share this exact instance, otherwise
// `resolve` panics on an id it does not own.
static SNAPSHOT_INTERNER: OnceLock<Interner> = OnceLock::new();

/// Intern a name via the snapshot module's shared interner. Use this (not a
/// fresh `Interner`) when constructing `VarDebugInfo` expected to be rendered
/// by `format_mir_body_verbose`.
#[allow(dead_code)]
pub(crate) fn intern_name(s: &str) -> glyim_core::Name {
    SNAPSHOT_INTERNER.get_or_init(Interner::new).intern(s)
}

/// format_mir_body.
pub fn format_mir_body(ctx: &TyCtx, body: &glyim_mir::Body) -> String {
    format_mir_body_impl(ctx, body, false)
}

/// Verbose variant (plan §24.3): annotates every place with its resolved type
/// (e.g. `$1: i32 = ...`) and each local with its debug-info variable name
/// (e.g. `$1 /* x */: i32`). The default `format_mir_body` stays terse.
pub fn format_mir_body_verbose(ctx: &TyCtx, body: &glyim_mir::Body) -> String {
    format_mir_body_impl(ctx, body, true)
}

fn format_mir_body_impl(ctx: &TyCtx, body: &glyim_mir::Body, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("fn {}():\n", body.owner));
    out.push_str(&format!("  arg_count: {}\n", body.arg_count));
    out.push_str(&format!(
        "  return_ty: {}\n",
        glyim_type::PrintTy::new(body.return_ty, ctx)
    ));
    out.push_str("  locals:\n");
    // Build a map from LocalIdx -> debug variable name for verbose annotation.
    let debug_names: HashMap<LocalIdx, String> = if verbose {
        body.var_debug_info
            .iter()
            .filter_map(|info| match &info.value {
                VarDebugInfoValue::Place(place) if place.projection.is_empty() => Some((
                    place.local,
                    SNAPSHOT_INTERNER
                        .get_or_init(Interner::new)
                        .resolve(info.name)
                        .to_string(),
                )),
                _ => None,
            })
            .collect()
    } else {
        HashMap::new()
    };
    for (idx, local) in body.locals.iter_enumerated() {
        let mut_str = match local.mutability {
            Mutability::Mut => "mut",
            Mutability::Not => "imm",
        };
        let dbg = if verbose {
            if let Some(name) = debug_names.get(&idx) {
                format!(" /* {} */", name)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        out.push_str(&format!(
            "    ${}{}: {} ({}){}\n",
            idx.to_raw(),
            dbg,
            glyim_type::PrintTy::new(local.ty, ctx),
            mut_str,
            if verbose {
                format!("  // {}", place_type_annotation(local, ctx))
            } else {
                String::new()
            },
        ));
    }
    if !body.var_debug_info.is_empty() {
        out.push_str("  debug:\n");
        for info in &body.var_debug_info {
            out.push_str(&format!("    {:?}: ", info.name));
            match &info.value {
                VarDebugInfoValue::Place(place) => {
                    out.push_str(&format!(
                        "{}\n",
                        format_place(place, verbose, ctx, &body.locals)
                    ));
                }
                VarDebugInfoValue::Const(c) => {
                    out.push_str(&format!("const {}\n", format_const(c)));
                }
            }
        }
    }
    for (idx, block) in body.basic_blocks.iter_enumerated() {
        let _cleanup_tag = if block.is_cleanup { " (cleanup)" } else { "" };
        out.push_str(&format!("  bb{}:\n", idx.to_raw()));
        for stmt in &block.statements {
            out.push_str(&format!(
                "    {}\n",
                format_statement(&stmt.kind, verbose, ctx, &body.locals)
            ));
        }
        out.push_str(&format!(
            "    -> {}\n",
            format_terminator(&block.terminator.kind, verbose, ctx, &body.locals)
        ));
    }
    out
}

fn place_type_annotation(local: &LocalDecl, ctx: &TyCtx) -> String {
    format!("{}", glyim_type::PrintTy::new(local.ty, ctx))
}

fn format_statement(
    kind: &glyim_mir::StatementKind,
    verbose: bool,
    ctx: &TyCtx,
    locals: &glyim_core::IndexVec<LocalIdx, LocalDecl>,
) -> String {
    match kind {
        glyim_mir::StatementKind::Assign(place, rvalue) => {
            format!(
                "{} = {}",
                format_place(place, verbose, ctx, locals),
                format_rvalue(rvalue, verbose, ctx, locals)
            )
        }
        glyim_mir::StatementKind::StorageLive(local) => {
            format!("StorageLive(${})", local.to_raw())
        }
        glyim_mir::StatementKind::StorageDead(local) => {
            format!("StorageDead(${})", local.to_raw())
        }
        glyim_mir::StatementKind::Nop => "Nop".to_string(),
    }
}

fn format_rvalue(
    rvalue: &glyim_mir::Rvalue,
    verbose: bool,
    ctx: &TyCtx,
    locals: &glyim_core::IndexVec<LocalIdx, LocalDecl>,
) -> String {
    match rvalue {
        glyim_mir::Rvalue::Use(op) => format!("Use({})", format_operand(op, verbose, ctx, locals)),
        glyim_mir::Rvalue::Ref(place, bk) => {
            format!(
                "Ref({}, {})",
                format_borrow_kind(bk),
                format_place(place, verbose, ctx, locals)
            )
        }
        glyim_mir::Rvalue::BinaryOp(op, pair) => {
            format!(
                "BinaryOp({:?}, {}, {})",
                op,
                format_operand(&pair.0, verbose, ctx, locals),
                format_operand(&pair.1, verbose, ctx, locals)
            )
        }
        glyim_mir::Rvalue::UnaryOp(op, val) => {
            format!(
                "UnaryOp({:?}, {})",
                op,
                format_operand(val, verbose, ctx, locals)
            )
        }
        glyim_mir::Rvalue::Aggregate(kind, ops) => {
            let args: Vec<String> = ops
                .iter()
                .map(|op| format_operand(op, verbose, ctx, locals))
                .collect();
            format!(
                "Aggregate({}, [{}])",
                format_aggregate_kind(kind),
                args.join(", ")
            )
        }
        glyim_mir::Rvalue::Discriminant(place) => {
            format!(
                "Discriminant({})",
                format_place(place, verbose, ctx, locals)
            )
        }
        glyim_mir::Rvalue::Len(place) => {
            format!("Len({})", format_place(place, verbose, ctx, locals))
        }
        glyim_mir::Rvalue::Cast(kind, op, ty) => {
            format!(
                "Cast({}, {}, {:?})",
                format_cast_kind(kind),
                format_operand(op, verbose, ctx, locals),
                ty
            )
        }
        glyim_mir::Rvalue::Repeat(op, count) => {
            format!(
                "Repeat({}, {})",
                format_operand(op, verbose, ctx, locals),
                format_const(count)
            )
        }
    }
}

fn format_aggregate_kind(kind: &glyim_mir::AggregateKind) -> String {
    match kind {
        glyim_mir::AggregateKind::Array(ty) => format!("Array({:?})", ty),
        glyim_mir::AggregateKind::Tuple => "Tuple".to_string(),
        glyim_mir::AggregateKind::Adt(adt_id, variant, substs) => {
            format!(
                "Adt({:?}, variant={:?}, substs={:?})",
                adt_id,
                variant.to_raw(),
                substs
            )
        }
        glyim_mir::AggregateKind::Closure(closure_id, substs) => {
            format!("Closure({:?}, substs={:?})", closure_id, substs)
        }
    }
}

fn format_borrow_kind(bk: &glyim_mir::BorrowKind) -> String {
    match bk {
        glyim_mir::BorrowKind::Shared => "Shared".to_string(),
        glyim_mir::BorrowKind::Unique => "Unique".to_string(),
        glyim_mir::BorrowKind::Mut {
            allow_two_phase_borrow,
        } => {
            format!("Mut(two_phase={})", allow_two_phase_borrow)
        }
    }
}

fn format_cast_kind(kind: &glyim_mir::CastKind) -> String {
    match kind {
        glyim_mir::CastKind::IntToInt => "IntToInt".to_string(),
        glyim_mir::CastKind::FloatToInt => "FloatToInt".to_string(),
        glyim_mir::CastKind::IntToFloat => "IntToFloat".to_string(),
        glyim_mir::CastKind::PtrToPtr => "PtrToPtr".to_string(),
        glyim_mir::CastKind::FnPtrToPtr => "FnPtrToPtr".to_string(),
        glyim_mir::CastKind::PtrToInt => "PtrToInt".to_string(),
        glyim_mir::CastKind::IntToPtr => "IntToPtr".to_string(),
        glyim_mir::CastKind::FloatToFloat => "FloatToFloat".to_string(),
    }
}

fn format_terminator(
    kind: &glyim_mir::TerminatorKind,
    verbose: bool,
    ctx: &TyCtx,
    locals: &glyim_core::IndexVec<LocalIdx, LocalDecl>,
) -> String {
    match kind {
        glyim_mir::TerminatorKind::Goto { target } => {
            format!("Goto(bb{})", target.to_raw())
        }
        glyim_mir::TerminatorKind::SwitchInt {
            discr,
            switch_ty,
            targets,
        } => {
            let branches: Vec<String> = targets
                .iter()
                .map(|(val, bb)| format!("{}: bb{}", val, bb.to_raw()))
                .collect();
            format!(
                "SwitchInt(discr={}, ty={:?}, [{}], otherwise=bb{})",
                format_operand(discr, verbose, ctx, locals),
                switch_ty,
                branches.join(", "),
                targets.otherwise().to_raw()
            )
        }
        glyim_mir::TerminatorKind::Return => "Return".to_string(),
        glyim_mir::TerminatorKind::Unreachable => "Unreachable".to_string(),
        glyim_mir::TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            cleanup,
        } => {
            let args_fmt: Vec<String> = args
                .iter()
                .map(|op| format_operand(op, verbose, ctx, locals))
                .collect();
            let target_fmt = match target {
                Some(bb) => format!("Some(bb{})", bb.to_raw()),
                None => "None".to_string(),
            };
            let cleanup_fmt = match cleanup {
                Some(bb) => format!("Some(bb{})", bb.to_raw()),
                None => "None".to_string(),
            };
            format!(
                "Call(func={}, args=[{}], dest={}, target={}, cleanup={})",
                format_operand(func, verbose, ctx, locals),
                args_fmt.join(", "),
                format_place(destination, verbose, ctx, locals),
                target_fmt,
                cleanup_fmt,
            )
        }
        glyim_mir::TerminatorKind::Assert {
            cond,
            expected,
            target,
            cleanup,
            msg,
        } => {
            let cleanup_fmt = match cleanup {
                Some(bb) => format!("Some(bb{})", bb.to_raw()),
                None => "None".to_string(),
            };
            format!(
                "Assert(cond={}, expected={}, target=bb{}, cleanup={}, msg={})",
                format_operand(cond, verbose, ctx, locals),
                expected,
                target.to_raw(),
                cleanup_fmt,
                format_assert_message(msg),
            )
        }
        glyim_mir::TerminatorKind::Drop {
            place,
            target,
            cleanup,
        } => {
            let cleanup_fmt = match cleanup {
                Some(bb) => format!("Some(bb{})", bb.to_raw()),
                None => "None".to_string(),
            };
            format!(
                "Drop({}, target=bb{}, cleanup={})",
                format_place(place, verbose, ctx, locals),
                target.to_raw(),
                cleanup_fmt,
            )
        }
    }
}

fn format_assert_message(msg: &glyim_mir::AssertMessage) -> String {
    match msg {
        glyim_mir::AssertMessage::Overflow(op) => format!("Overflow({:?})", op),
        glyim_mir::AssertMessage::DivisionByZero => "DivisionByZero".to_string(),
        glyim_mir::AssertMessage::RemainderByZero => "RemainderByZero".to_string(),
        glyim_mir::AssertMessage::BoundsCheck => "BoundsCheck".to_string(),
    }
}

fn format_place(
    place: &glyim_mir::Place,
    verbose: bool,
    ctx: &TyCtx,
    locals: &glyim_core::IndexVec<LocalIdx, LocalDecl>,
) -> String {
    let base = format!("${}", place.local.to_raw());
    let mut s = if place.projection.is_empty() {
        base
    } else {
        let proj: Vec<String> = place
            .projection
            .iter()
            .map(format_projection_elem)
            .collect();
        format!("{}.{}", base, proj.join("."))
    };
    if verbose {
        let ty = place.ty(ctx, locals);
        s.push_str(&format!(": {}", glyim_type::PrintTy::new(ty, ctx)));
    }
    s
}

fn format_projection_elem(elem: &glyim_mir::ProjectionElem) -> String {
    match elem {
        glyim_mir::ProjectionElem::Deref => "Deref".to_string(),
        glyim_mir::ProjectionElem::Field(idx) => format!("Field({})", idx.to_raw()),
        glyim_mir::ProjectionElem::Index(idx) => format!("Index(${})", idx.to_raw()),
        glyim_mir::ProjectionElem::Downcast(variant) => {
            format!("Downcast(variant={})", variant.to_raw())
        }
        glyim_mir::ProjectionElem::ConstantIndex {
            offset,
            min_length,
            from_end,
        } => format!(
            "ConstantIndex(offset={}, min_length={}, from_end={})",
            offset, min_length, from_end
        ),
        glyim_mir::ProjectionElem::Subslice { from, to, from_end } => {
            format!("Subslice(from={}, to={}, from_end={})", from, to, from_end)
        }
    }
}

fn format_operand(
    op: &glyim_mir::Operand,
    verbose: bool,
    ctx: &TyCtx,
    locals: &glyim_core::IndexVec<LocalIdx, LocalDecl>,
) -> String {
    match op {
        glyim_mir::Operand::Copy(place) => {
            format!("Copy({})", format_place(place, verbose, ctx, locals))
        }
        glyim_mir::Operand::Move(place) => {
            format!("Move({})", format_place(place, verbose, ctx, locals))
        }
        glyim_mir::Operand::Constant(c) => format!("Const({})", format_const(c)),
    }
}

fn format_const(c: &glyim_mir::MirConst) -> String {
    match &c.kind {
        glyim_mir::MirConstKind::Int(v) => format!("int({})", v),
        glyim_mir::MirConstKind::Uint(v) => format!("uint({})", v),
        glyim_mir::MirConstKind::FloatBits(bits) => format!("float_bits({})", bits),
        glyim_mir::MirConstKind::Bool(v) => format!("bool({})", v),
        glyim_mir::MirConstKind::Char(v) => format!("char('{}')", v.escape_default()),
        glyim_mir::MirConstKind::String(name) => format!("string({:?})", name),
        glyim_mir::MirConstKind::Unit => "()".to_string(),
        glyim_mir::MirConstKind::Fn(def_id, substs) => format!("fn({:?}, {:?})", def_id, substs),
        glyim_mir::MirConstKind::ConstRef(def_id, substs) => {
            format!("const_ref({:?}, {:?})", def_id, substs)
        }
        glyim_mir::MirConstKind::Aggregate(elems) => {
            let parts: Vec<String> = elems.iter().map(format_const).collect();
            format!("aggregate([{}])", parts.join(", "))
        }
        glyim_mir::MirConstKind::Error => "<error>".to_string(),
        glyim_mir::MirConstKind::VirtualMethod {
            trait_def_id,
            method_name,
        } => format!("virtual_method({:?}::{:?})", trait_def_id, method_name),
    }
}

/// format_def_map.
pub fn format_def_map(def_map: &glyim_def_map::CrateDefMap) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "CrateDefMap (root: {:?}, krate: {:?})\n",
        def_map.root, def_map.krate
    ));
    for (idx, module) in def_map.modules.iter_enumerated() {
        out.push_str(&format!("  module {:?}:\n", idx));
        out.push_str(&format!("    parent: {:?}\n", module.parent));
        out.push_str(&format!(
            "    origin: {}\n",
            format_module_origin(&module.origin)
        ));
        out.push_str(&format!("    children: {}\n", module.children.len()));
        out.push_str(&format!("    scope.types: {}\n", module.scope.types.len()));
        out.push_str(&format!(
            "    scope.values: {}\n",
            module.scope.values.len()
        ));
        out.push_str(&format!(
            "    scope.macros: {}\n",
            module.scope.macros.len()
        ));
    }
    out
}

fn format_module_origin(origin: &glyim_def_map::ModuleOrigin) -> String {
    match origin {
        glyim_def_map::ModuleOrigin::CrateRoot => "CrateRoot".to_string(),
        glyim_def_map::ModuleOrigin::File { file_id } => format!("File({:?})", file_id),
        glyim_def_map::ModuleOrigin::Inline { span } => format!("Inline({:?})", span),
    }
}
