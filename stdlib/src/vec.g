// Glyim Standard Library — Vec<T>
//
// A growable array type backed by heap allocation.
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. Generic *mut T not supported — would need *mut u8 with unsafe casts
//   §2. Pointer load/store (dereferencing) not supported for generic types
//   §3. __size_of::<T>() works but pointer arithmetic on *mut u8 needs
//      byte offsets computed manually: ptr + (index * __size_of::<T>())
//   §4. No Drop/destructor — Vec memory leaks when it goes out of scope
//   §5. Struct method call resolution broken (Vec::push can't be called as v.push())
//
// DESIGN (for when blockers are resolved):
//   - Internal representation: struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
//   - All pointer math done in bytes using *mut u8, with manual size_of multiplication
//   - Growth factor: 2x (or 8 for initial allocation)
//   - OOM: glyim_alloc returns null → abort (handled by wrapper)
//   - Drop: calls glyim_free on data pointer

// struct Vec<T> {
//     data: *mut u8,
//     len: i64,
//     cap: i64,
// }
//
// fn Vec::new<T>() -> Vec<T> { Vec { data: null_mut::<u8>(), len: 0, cap: 0 } }
// fn Vec::push<T>(self: *mut Vec<T>, value: T) { ... }
// fn Vec::pop<T>(self: *mut Vec<T>) -> Option<T> { ... }
// fn Vec::get<T>(self: *Vec<T>, index: i64) -> Option<T> { ... }
// fn Vec::len<T>(self: *Vec<T>) -> i64 { self.len }
// fn Vec::is_empty<T>(self: *Vec<T>) -> bool { self.len == 0 }
// fn Vec::drop<T>(self: *mut Vec<T>) { ... }
// fn Vec::capacity<T>(self: *Vec<T>) -> i64 { self.cap }
