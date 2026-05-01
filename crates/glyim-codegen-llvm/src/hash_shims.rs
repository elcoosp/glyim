use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::AddressSpace;
use inkwell::IntPredicate;

/// Emit runtime hash functions: glyim_hash_bytes (FNV‑1a) and glyim_hash_seed.
pub fn emit_hash_shims<'ctx>(context: &'ctx Context, module: &Module<'ctx>, no_std: bool) {
    if no_std {
        return;
    }

    let i64_type = context.i64_type();
    let ptr_type = context.ptr_type(AddressSpace::from(0u16));

    // ── glyim_hash_seed() -> i64 ───────────────────────────────
    let seed_fn_type = i64_type.fn_type(&[], false);
    let seed_fn = module.add_function("glyim_hash_seed", seed_fn_type, None);
    let entry = context.append_basic_block(seed_fn, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry);
    let zero = i64_type.const_int(0, false);
    builder.build_return(Some(&zero)).unwrap();

    // ── glyim_hash_bytes(ptr: *const u8, len: i64) -> i64 ─────
    let hash_fn_type = i64_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let hash_fn = module.add_function("glyim_hash_bytes", hash_fn_type, None);

    let entry_bb = context.append_basic_block(hash_fn, "entry");
    let loop_cond_bb = context.append_basic_block(hash_fn, "loop.cond");
    let loop_body_bb = context.append_basic_block(hash_fn, "loop.body");
    let exit_bb = context.append_basic_block(hash_fn, "exit");

    // ── entry ─────────────────────────────────────────────────
    builder.position_at_end(entry_bb);
    let data = hash_fn.get_nth_param(0).unwrap().into_pointer_value();
    let len = hash_fn.get_nth_param(1).unwrap().into_int_value();

    // hash = FNV_OFFSET_BASIS
    let basis_val = i64_type.const_int(0xcbf29ce484222325_u64, false);
    let hash_ptr = builder.build_alloca(i64_type, "hash").unwrap();
    builder.build_store(hash_ptr, basis_val).unwrap();

    // i = 0
    let i_ptr = builder.build_alloca(i64_type, "i").unwrap();
    builder
        .build_store(i_ptr, i64_type.const_int(0, false))
        .unwrap();

    builder.build_unconditional_branch(loop_cond_bb).unwrap();

    // ── loop.cond ─────────────────────────────────────────────
    builder.position_at_end(loop_cond_bb);
    let i_val = builder
        .build_load(i64_type, i_ptr, "i.ld")
        .unwrap()
        .into_int_value();
    let cond = builder
        .build_int_compare(IntPredicate::SLT, i_val, len, "cmp")
        .unwrap();
    builder
        .build_conditional_branch(cond, loop_body_bb, exit_bb)
        .unwrap();

    // ── loop.body ─────────────────────────────────────────────
    builder.position_at_end(loop_body_bb);

    // byte = data[i]
    let byte_ptr = unsafe {
        builder
            .build_gep(ptr_type, data, &[i_val], "byte_ptr")
            .unwrap()
    };
    let byte_val = builder
        .build_load(context.i8_type(), byte_ptr, "byte")
        .unwrap()
        .into_int_value();

    // hash ^= byte (zero-extended to i64)
    let byte_i64 = builder
        .build_int_z_extend(byte_val, i64_type, "byte64")
        .unwrap();
    let cur_hash = builder
        .build_load(i64_type, hash_ptr, "cur")
        .unwrap()
        .into_int_value();
    let xored = builder.build_xor(cur_hash, byte_i64, "xored").unwrap();

    // hash *= FNV_PRIME
    let prime = i64_type.const_int(0x100000001b3_u64, false);
    let mul = builder.build_int_mul(xored, prime, "mul").unwrap();
    builder.build_store(hash_ptr, mul).unwrap();

    // i += 1
    let inc = builder
        .build_int_add(i_val, i64_type.const_int(1, false), "inc")
        .unwrap();
    builder.build_store(i_ptr, inc).unwrap();

    builder.build_unconditional_branch(loop_cond_bb).unwrap();

    // ── exit ──────────────────────────────────────────────────
    builder.position_at_end(exit_bb);
    let final_hash = builder
        .build_load(i64_type, hash_ptr, "final")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&final_hash)).unwrap();
}
