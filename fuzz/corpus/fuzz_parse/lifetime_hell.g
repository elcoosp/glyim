fn f<'a, 'b, 'c, 'd>(x: &'a &'b &'c &'d i32) -> &'a i32 { x }
