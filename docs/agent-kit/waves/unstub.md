## Merge Overview (Based on Final Streams)

**Assumption:** Each wave’s tasks modify **different crates** (no two tasks in the same wave touch the same crate). If a crate has multiple changes within a wave, they must be **combined into a single task** before merging.

| Wave | Merge Condition | What to Merge |
|------|----------------|----------------|
| **1** | All 7 tasks pass CI | `W1-C01` (type flags), `W1-C02` (lexer diagnostics), `W1-C03` (interp bitwise/shift), `W1-C04` (interp len/discriminant), `W1-C05` (runtime alloc/panic), `W1-C06` (runtime env/args), `W1-C07` (runtime process) |
| **2** | All 7 tasks pass CI | `W2-C01` (parser index), `W2-C02` (parser patterns), `W2-C03` (macros file/line/col), `W2-C04` (macros env/include), `W2-C05` (macros concat/stringify), `W2-C06` (HIR index), `W2-C07` (HIR patterns) |
| **3** | All 7 tasks pass CI | `W3-C01` (typeck match guards), `W3-C02` (typeck patterns), `W3-C03` (coercions), `W3-C04` (typeck struct/spread), `W3-C05` (typeck index), `W3-C06` (typeck range), `W3-C07` (HIR field access) |
| **4** | All 7 tasks pass CI | `W4-C01` (MIR index), `W4-C02` (MIR or-patterns), `W4-C03` (MIR ranges), `W4-C04` (MIR slice patterns), `W4-C05` (MIR guards), `W4-C06` (MIR struct/spread), `W4-C07` (optimizations) |
| **5** | All 7 tasks pass CI | `W5-C01` (borrowck index), `W5-C02` (borrowck slice patterns), `W5-C03` (LLVM slice), `W5-C04` (bytecode ops), `W5-C05` (runtime fs), `W5-C06` (runtime TCP), `W5-C07` (runtime UDP/thread/time) |

**Critical Rule:**  
Merge **all tasks of a wave together** into `main` only after **every task in that wave has passed CI**.  
Do **not** merge a later wave before the previous wave is fully merged.

**Conflict Prevention:**  
If any wave has two tasks modifying the same crate (e.g., `glyim-mir-interp` tasks in Wave 1), **combine them into a single task** assigned to one agent. The streams above already assume that; if not, adjust accordingly.
