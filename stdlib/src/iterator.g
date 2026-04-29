// Glyim Standard Library — Iterator Trait
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. impl Trait for Type syntax parses but method resolution broken
//   §2. for ... in expr syntax doesn't exist
//   §3. Generic trait dispatch not implemented
//
// DESIGN:
//   trait Iterator<T> {
//       fn next(self: *mut Self) -> Option<T>;
//   }
//
// When working: for item in collection desugars to:
//   let mut iter = collection.iter();
//   loop { match iter.next() { Some(v) => ..., None => break } }
