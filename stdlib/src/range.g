// Glyim Standard Library — Range
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. Depends on Iterator trait (see iterator.glyim blockers)
//   §2. impl Iterator<i64> for Range<i64> needs working impl resolution
//   §3. Generic impl blocks not fully supported
//
// DESIGN:
//   struct Range<T> { start: T, end: T, current: T }
//
//   impl Iterator<i64> for Range<i64> {
//       fn next(self: *mut Range<i64>) -> Option<i64> {
//           if self.current >= self.end { return None; }
//           let val = self.current;
//           self.current = self.current + 1;
//           Some(val)
//       }
//   }
//
// Usage: see iterator.glyim
