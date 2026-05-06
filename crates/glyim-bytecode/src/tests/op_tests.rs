use crate::op::BytecodeOp;

#[test] fn push_i64_roundtrip() { let op = BytecodeOp::PushI64(42); let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn push_f64_roundtrip() { let op = BytecodeOp::PushF64(3.14); let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn load_local_roundtrip() { let op = BytecodeOp::LoadLocal(3); let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn binop_roundtrip() { let op = BytecodeOp::BinOp(0); /* BinOp tag 0 = Add */ let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn call_roundtrip() { let op = BytecodeOp::Call { name: "add".into(), arg_count: 2 }; let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn jump_roundtrip() { let op = BytecodeOp::Jump(10); let bytes = postcard::to_allocvec(&op).unwrap(); let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap(); assert_eq!(op, restored); }
#[test] fn debug_contains_value() { let op = BytecodeOp::PushI64(99); assert!(format!("{:?}", op).contains("99")); }
#[test] fn is_send_sync() { fn b<T: Send+Sync>() {} b::<BytecodeOp>(); }
