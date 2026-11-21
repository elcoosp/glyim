//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn main() -> i32 {
    let s0 = format!("k0=20");
    if s0.as_str().len() != 5 { return 1; }
    let p0 = format!("{56} + {12}");
    if p0.as_str().len() != 7 { return 1; }
    let s1 = format!("k1=9");
    if s1.as_str().len() != 4 { return 2; }
    let s2 = format!("k2=20");
    if s2.as_str().len() != 5 { return 3; }
    let s3 = format!("k3=60");
    if s3.as_str().len() != 5 { return 4; }
    let p3 = format!("{34} + {13}");
    if p3.as_str().len() != 7 { return 4; }
    let s4 = format!("k4=65");
    if s4.as_str().len() != 5 { return 5; }
    let s5 = format!("k5=21");
    if s5.as_str().len() != 5 { return 6; }
    let s6 = format!("k6=53");
    if s6.as_str().len() != 5 { return 7; }
    let p6 = format!("{40} + {45}");
    if p6.as_str().len() != 7 { return 7; }
    let s7 = format!("k7=13");
    if s7.as_str().len() != 5 { return 8; }
    let s8 = format!("k8=64");
    if s8.as_str().len() != 5 { return 9; }
    let s9 = format!("k9=34");
    if s9.as_str().len() != 5 { return 10; }
    let p9 = format!("{40} + {56}");
    if p9.as_str().len() != 7 { return 10; }
    let s10 = format!("k10=14");
    if s10.as_str().len() != 6 { return 11; }
    let s11 = format!("k11=32");
    if s11.as_str().len() != 6 { return 12; }
    let s12 = format!("k12=67");
    if s12.as_str().len() != 6 { return 13; }
    let p12 = format!("{64} + {12}");
    if p12.as_str().len() != 7 { return 13; }
    let s13 = format!("k13=20");
    if s13.as_str().len() != 6 { return 14; }
    let s14 = format!("k14=38");
    if s14.as_str().len() != 6 { return 15; }
    let s15 = format!("k15=18");
    if s15.as_str().len() != 6 { return 16; }
    let p15 = format!("{9} + {44}");
    if p15.as_str().len() != 6 { return 16; }
    let s16 = format!("k16=51");
    if s16.as_str().len() != 6 { return 17; }
    let s17 = format!("k17=36");
    if s17.as_str().len() != 6 { return 18; }
    let s18 = format!("k18=12");
    if s18.as_str().len() != 6 { return 19; }
    let p18 = format!("{53} + {15}");
    if p18.as_str().len() != 7 { return 19; }
    let s19 = format!("k19=45");
    if s19.as_str().len() != 6 { return 20; }
    let s20 = format!("k20=37");
    if s20.as_str().len() != 6 { return 21; }
    let s21 = format!("k21=7");
    if s21.as_str().len() != 5 { return 22; }
    let p21 = format!("{60} + {28}");
    if p21.as_str().len() != 7 { return 22; }
    let s22 = format!("k22=33");
    if s22.as_str().len() != 6 { return 23; }
    let s23 = format!("k23=47");
    if s23.as_str().len() != 6 { return 24; }
    let s24 = format!("k24=8");
    if s24.as_str().len() != 5 { return 25; }
    let p24 = format!("{34} + {13}");
    if p24.as_str().len() != 7 { return 25; }
    let s25 = format!("k25=31");
    if s25.as_str().len() != 6 { return 26; }
    let s26 = format!("k26=26");
    if s26.as_str().len() != 6 { return 27; }
    let s27 = format!("k27=61");
    if s27.as_str().len() != 6 { return 28; }
    let p27 = format!("{43} + {32}");
    if p27.as_str().len() != 7 { return 28; }
    let s28 = format!("k28=54");
    if s28.as_str().len() != 6 { return 29; }
    let s29 = format!("k29=64");
    if s29.as_str().len() != 6 { return 30; }
    let s30 = format!("k30=25");
    if s30.as_str().len() != 6 { return 31; }
    let p30 = format!("{46} + {48}");
    if p30.as_str().len() != 7 { return 31; }
    let s31 = format!("k31=42");
    if s31.as_str().len() != 6 { return 32; }
    println!("done");
    0
}
