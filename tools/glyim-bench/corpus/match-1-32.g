//@ mode: run-pass
//@ expect-exit: 206
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
enum Op { Add, Sub, Mul }

fn classify(op: Op) -> i32 { match op { Op::Add => 3, Op::Sub => 5, Op::Mul => 11 } }

fn main() -> i32 {
    let r0 = classify(Op::Sub);
    let r1 = r0 + classify(Op::Mul);
    let r2 = r1 + classify(Op::Add);
    let r3 = r2 + classify(Op::Sub);
    let r4 = r3 + classify(Op::Mul);
    let r5 = r4 + classify(Op::Add);
    let r6 = r5 + classify(Op::Sub);
    let r7 = r6 + classify(Op::Mul);
    let r8 = r7 + classify(Op::Add);
    let r9 = r8 + classify(Op::Sub);
    let r10 = r9 + classify(Op::Mul);
    let r11 = r10 + classify(Op::Add);
    let r12 = r11 + classify(Op::Sub);
    let r13 = r12 + classify(Op::Mul);
    let r14 = r13 + classify(Op::Add);
    let r15 = r14 + classify(Op::Sub);
    let r16 = r15 + classify(Op::Mul);
    let r17 = r16 + classify(Op::Add);
    let r18 = r17 + classify(Op::Sub);
    let r19 = r18 + classify(Op::Mul);
    let r20 = r19 + classify(Op::Add);
    let r21 = r20 + classify(Op::Sub);
    let r22 = r21 + classify(Op::Mul);
    let r23 = r22 + classify(Op::Add);
    let r24 = r23 + classify(Op::Sub);
    let r25 = r24 + classify(Op::Mul);
    let r26 = r25 + classify(Op::Add);
    let r27 = r26 + classify(Op::Sub);
    let r28 = r27 + classify(Op::Mul);
    let r29 = r28 + classify(Op::Add);
    let r30 = r29 + classify(Op::Sub);
    let r31 = r30 + classify(Op::Mul);
    r31
}
