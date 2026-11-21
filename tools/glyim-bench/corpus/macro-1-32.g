//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn main() -> i32 {
    let s0 = format!("k0=56");
    if s0.as_str().len() != 5 { return 1; }
    let p0 = format!("{12} + {34}");
    if p0.as_str().len() != 7 { return 1; }
    let s1 = format!("k1=58");
    if s1.as_str().len() != 5 { return 2; }
    let s2 = format!("k2=64");
    if s2.as_str().len() != 5 { return 3; }
    let s3 = format!("k3=34");
    if s3.as_str().len() != 5 { return 4; }
    let p3 = format!("{13} + {14}");
    if p3.as_str().len() != 7 { return 4; }
    let s4 = format!("k4=45");
    if s4.as_str().len() != 5 { return 5; }
    let s5 = format!("k5=43");
    if s5.as_str().len() != 5 { return 6; }
    let s6 = format!("k6=40");
    if s6.as_str().len() != 5 { return 7; }
    let p6 = format!("{45} + {21}");
    if p6.as_str().len() != 7 { return 7; }
    let s7 = format!("k7=28");
    if s7.as_str().len() != 5 { return 8; }
    let s8 = format!("k8=39");
    if s8.as_str().len() != 5 { return 9; }
    let s9 = format!("k9=40");
    if s9.as_str().len() != 5 { return 10; }
    let p9 = format!("{56} + {38}");
    if p9.as_str().len() != 7 { return 10; }
    let s10 = format!("k10=18");
    if s10.as_str().len() != 6 { return 11; }
    let s11 = format!("k11=17");
    if s11.as_str().len() != 6 { return 12; }
    let s12 = format!("k12=64");
    if s12.as_str().len() != 6 { return 13; }
    let p12 = format!("{12} + {50}");
    if p12.as_str().len() != 7 { return 13; }
    let s13 = format!("k13=45");
    if s13.as_str().len() != 6 { return 14; }
    let s14 = format!("k14=42");
    if s14.as_str().len() != 6 { return 15; }
    let s15 = format!("k15=9");
    if s15.as_str().len() != 5 { return 16; }
    let p15 = format!("{44} + {47}");
    if p15.as_str().len() != 7 { return 16; }
    let s16 = format!("k16=52");
    if s16.as_str().len() != 6 { return 17; }
    let s17 = format!("k17=39");
    if s17.as_str().len() != 6 { return 18; }
    let s18 = format!("k18=53");
    if s18.as_str().len() != 6 { return 19; }
    let p18 = format!("{15} + {43}");
    if p18.as_str().len() != 7 { return 19; }
    let s19 = format!("k19=31");
    if s19.as_str().len() != 6 { return 20; }
    let s20 = format!("k20=9");
    if s20.as_str().len() != 5 { return 21; }
    let s21 = format!("k21=60");
    if s21.as_str().len() != 6 { return 22; }
    let p21 = format!("{28} + {24}");
    if p21.as_str().len() != 7 { return 22; }
    let s22 = format!("k22=45");
    if s22.as_str().len() != 6 { return 23; }
    let s23 = format!("k23=10");
    if s23.as_str().len() != 6 { return 24; }
    let s24 = format!("k24=34");
    if s24.as_str().len() != 6 { return 25; }
    let p24 = format!("{13} + {8}");
    if p24.as_str().len() != 6 { return 25; }
    let s25 = format!("k25=67");
    if s25.as_str().len() != 6 { return 26; }
    let s26 = format!("k26=7");
    if s26.as_str().len() != 5 { return 27; }
    let s27 = format!("k27=43");
    if s27.as_str().len() != 6 { return 28; }
    let p27 = format!("{32} + {65}");
    if p27.as_str().len() != 7 { return 28; }
    let s28 = format!("k28=57");
    if s28.as_str().len() != 6 { return 29; }
    let s29 = format!("k29=20");
    if s29.as_str().len() != 6 { return 30; }
    let s30 = format!("k30=46");
    if s30.as_str().len() != 6 { return 31; }
    let p30 = format!("{48} + {58}");
    if p30.as_str().len() != 7 { return 31; }
    let s31 = format!("k31=56");
    if s31.as_str().len() != 6 { return 32; }
    println!("done");
    0
}
