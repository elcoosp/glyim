// Glyim Standard Library — String
//
// A UTF-8 encoded, growable string type.
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. Depends on Vec<u8> (see vec.glyim blockers)
//   §2. Needs &str.as_bytes() returning indexable byte sequence
//   §3. No way to convert Vec<u8> back to &str without unsafe transmute
//
// DESIGN:
//   struct String { vec: Vec<u8> }
//
// fn String::from(s: &str) -> String { ... }
// fn String::len(self: *String) -> i64 { self.vec.len() }
// fn String::push_str(self: *mut String, s: &str) { ... }
// fn String::as_str(self: *String) -> &str { ... }
// fn String::is_empty(self: *String) -> bool { self.vec.is_empty() }
