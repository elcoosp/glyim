//@ mode: run-pass
//@ expect-exit: 1127
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
trait Read {
    fn get(&mut self) -> i32;
}

struct W0 { r: i32 }

impl Read for W0 {
    fn get(&mut self) -> i32 { self.r }
}

struct W1 { r: i32 }

impl Read for W1 {
    fn get(&mut self) -> i32 { self.r }
}

struct W2 { r: i32 }

impl Read for W2 {
    fn get(&mut self) -> i32 { self.r }
}

struct W3 { r: i32 }

impl Read for W3 {
    fn get(&mut self) -> i32 { self.r }
}

struct W4 { r: i32 }

impl Read for W4 {
    fn get(&mut self) -> i32 { self.r }
}

struct W5 { r: i32 }

impl Read for W5 {
    fn get(&mut self) -> i32 { self.r }
}

struct W6 { r: i32 }

impl Read for W6 {
    fn get(&mut self) -> i32 { self.r }
}

struct W7 { r: i32 }

impl Read for W7 {
    fn get(&mut self) -> i32 { self.r }
}

struct W8 { r: i32 }

impl Read for W8 {
    fn get(&mut self) -> i32 { self.r }
}

struct W9 { r: i32 }

impl Read for W9 {
    fn get(&mut self) -> i32 { self.r }
}

struct W10 { r: i32 }

impl Read for W10 {
    fn get(&mut self) -> i32 { self.r }
}

struct W11 { r: i32 }

impl Read for W11 {
    fn get(&mut self) -> i32 { self.r }
}

struct W12 { r: i32 }

impl Read for W12 {
    fn get(&mut self) -> i32 { self.r }
}

struct W13 { r: i32 }

impl Read for W13 {
    fn get(&mut self) -> i32 { self.r }
}

struct W14 { r: i32 }

impl Read for W14 {
    fn get(&mut self) -> i32 { self.r }
}

struct W15 { r: i32 }

impl Read for W15 {
    fn get(&mut self) -> i32 { self.r }
}

struct W16 { r: i32 }

impl Read for W16 {
    fn get(&mut self) -> i32 { self.r }
}

struct W17 { r: i32 }

impl Read for W17 {
    fn get(&mut self) -> i32 { self.r }
}

struct W18 { r: i32 }

impl Read for W18 {
    fn get(&mut self) -> i32 { self.r }
}

struct W19 { r: i32 }

impl Read for W19 {
    fn get(&mut self) -> i32 { self.r }
}

struct W20 { r: i32 }

impl Read for W20 {
    fn get(&mut self) -> i32 { self.r }
}

struct W21 { r: i32 }

impl Read for W21 {
    fn get(&mut self) -> i32 { self.r }
}

struct W22 { r: i32 }

impl Read for W22 {
    fn get(&mut self) -> i32 { self.r }
}

struct W23 { r: i32 }

impl Read for W23 {
    fn get(&mut self) -> i32 { self.r }
}

struct W24 { r: i32 }

impl Read for W24 {
    fn get(&mut self) -> i32 { self.r }
}

struct W25 { r: i32 }

impl Read for W25 {
    fn get(&mut self) -> i32 { self.r }
}

struct W26 { r: i32 }

impl Read for W26 {
    fn get(&mut self) -> i32 { self.r }
}

struct W27 { r: i32 }

impl Read for W27 {
    fn get(&mut self) -> i32 { self.r }
}

struct W28 { r: i32 }

impl Read for W28 {
    fn get(&mut self) -> i32 { self.r }
}

struct W29 { r: i32 }

impl Read for W29 {
    fn get(&mut self) -> i32 { self.r }
}

struct W30 { r: i32 }

impl Read for W30 {
    fn get(&mut self) -> i32 { self.r }
}

struct W31 { r: i32 }

impl Read for W31 {
    fn get(&mut self) -> i32 { self.r }
}

fn main() -> i32 {
    let mut w0 = W0 { r: 20 };
    let s0 = w0.get();
    let mut w1 = W1 { r: 9 };
    let s1 = s0 + w1.get();
    let mut w2 = W2 { r: 20 };
    let s2 = s1 + w2.get();
    let mut w3 = W3 { r: 60 };
    let s3 = s2 + w3.get();
    let mut w4 = W4 { r: 65 };
    let s4 = s3 + w4.get();
    let mut w5 = W5 { r: 21 };
    let s5 = s4 + w5.get();
    let mut w6 = W6 { r: 53 };
    let s6 = s5 + w6.get();
    let mut w7 = W7 { r: 13 };
    let s7 = s6 + w7.get();
    let mut w8 = W8 { r: 64 };
    let s8 = s7 + w8.get();
    let mut w9 = W9 { r: 34 };
    let s9 = s8 + w9.get();
    let mut w10 = W10 { r: 14 };
    let s10 = s9 + w10.get();
    let mut w11 = W11 { r: 32 };
    let s11 = s10 + w11.get();
    let mut w12 = W12 { r: 67 };
    let s12 = s11 + w12.get();
    let mut w13 = W13 { r: 20 };
    let s13 = s12 + w13.get();
    let mut w14 = W14 { r: 38 };
    let s14 = s13 + w14.get();
    let mut w15 = W15 { r: 18 };
    let s15 = s14 + w15.get();
    let mut w16 = W16 { r: 51 };
    let s16 = s15 + w16.get();
    let mut w17 = W17 { r: 36 };
    let s17 = s16 + w17.get();
    let mut w18 = W18 { r: 12 };
    let s18 = s17 + w18.get();
    let mut w19 = W19 { r: 45 };
    let s19 = s18 + w19.get();
    let mut w20 = W20 { r: 37 };
    let s20 = s19 + w20.get();
    let mut w21 = W21 { r: 7 };
    let s21 = s20 + w21.get();
    let mut w22 = W22 { r: 33 };
    let s22 = s21 + w22.get();
    let mut w23 = W23 { r: 47 };
    let s23 = s22 + w23.get();
    let mut w24 = W24 { r: 8 };
    let s24 = s23 + w24.get();
    let mut w25 = W25 { r: 31 };
    let s25 = s24 + w25.get();
    let mut w26 = W26 { r: 26 };
    let s26 = s25 + w26.get();
    let mut w27 = W27 { r: 61 };
    let s27 = s26 + w27.get();
    let mut w28 = W28 { r: 54 };
    let s28 = s27 + w28.get();
    let mut w29 = W29 { r: 64 };
    let s29 = s28 + w29.get();
    let mut w30 = W30 { r: 25 };
    let s30 = s29 + w30.get();
    let mut w31 = W31 { r: 42 };
    let s31 = s30 + w31.get();
    s31
}
