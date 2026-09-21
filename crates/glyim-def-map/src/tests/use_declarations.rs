use glyim_core::def_id::CrateId;
use glyim_core::primitives::Visibility;
use glyim_test::FrontendTester;

fn get_def_map(source: &str) -> (crate::CrateDefMap, Vec<glyim_diag::GlyimDiagnostic>) {
    let trace = FrontendTester::new(source).run();

    if !trace.parse_diagnostics.is_empty() {
        println!("--- PARSE DIAGNOSTICS ---");
        for d in &trace.parse_diagnostics {
            println!("{:?}", d);
        }
        println!("--- END PARSE DIAGNOSTICS ---");
    }

    let root = trace.parse_tree.expect("Parse tree should exist");
    crate::build_def_map(
        &root,
        CrateId::from_raw(0),
        glyim_core::interner::Interner::new(),
    )
}

#[test]
fn u08_t01_std_io_read_imports_read() {
    let source = r#"
        mod std {
            pub mod io {
                pub struct Read;
            }
        }
        use std::io::Read;
    "#;

    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let read_name = def_map.interner.intern("Read");

    let resolved = def_map.modules[root_mod].resolve(read_name);
    assert!(resolved.is_some(), "Read should be in root scope");

    let (_id, vis) = resolved.unwrap();
    // `use std::io::Read;` without `pub` makes the binding private (Inherited)
    assert_eq!(
        vis,
        Visibility::Inherited,
        "Read should be inherited (private) by default use"
    );
}

#[test]
fn u08_t02_crate_foo_imports_foo() {
    let source = r#"
        pub struct Foo;
        use crate::Foo;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let foo_name = def_map.interner.intern("Foo");

    let resolved = def_map.modules[root_mod].resolve(foo_name);
    assert!(resolved.is_some(), "Foo should be resolvable");
}

#[test]
fn u08_t03_self_bar_imports_bar() {
    let source = r#"
        pub struct Bar;
        use self::Bar;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let bar_name = def_map.interner.intern("Bar");
    let resolved = def_map.modules[root_mod].resolve(bar_name);
    assert!(resolved.is_some(), "Bar should be resolvable");
}

#[test]
fn u08_t04_super_baz_imports_baz() {
    let source = r#"
        pub struct Baz;
        mod child {
            use super::Baz;
            fn check() {}
        }
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let child_name = def_map.interner.intern("child");
    let child_id = def_map.modules[root_mod]
        .children
        .iter()
        .find(|(n, _)| *n == child_name)
        .map(|(_, id)| *id)
        .expect("Child module should exist");

    let baz_name = def_map.interner.intern("Baz");
    let resolved = def_map.modules[child_id].resolve(baz_name);
    assert!(
        resolved.is_some(),
        "Baz should be resolvable in child module via super"
    );
}

#[test]
fn u08_t05_nested_imports() {
    let source = r#"
        mod std {
            pub mod io {
                pub struct Read;
                pub struct Write;
            }
        }
        use std::io::{Read, Write};
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let read_name = def_map.interner.intern("Read");
    let write_name = def_map.interner.intern("Write");

    assert!(
        def_map.modules[root_mod].resolve(read_name).is_some(),
        "Read should be imported"
    );
    assert!(
        def_map.modules[root_mod].resolve(write_name).is_some(),
        "Write should be imported"
    );
}

#[test]
fn u08_t06_glob_import() {
    let source = r#"
        mod std {
            pub mod io {
                pub struct Read;
                pub struct Write;
            }
        }
        use std::io::*;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let read_name = def_map.interner.intern("Read");
    let write_name = def_map.interner.intern("Write");

    assert!(
        def_map.modules[root_mod].resolve(read_name).is_some(),
        "Glob should import Read"
    );
    assert!(
        def_map.modules[root_mod].resolve(write_name).is_some(),
        "Glob should import Write"
    );
}

#[test]
fn u08_t07_deeply_nested_path() {
    let source = r#"
        mod std {
            pub mod io {
                pub mod fs {
                    pub struct File;
                }
            }
        }
        use std::io::fs::File;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let file_name = def_map.interner.intern("File");

    assert!(
        def_map.modules[root_mod].resolve(file_name).is_some(),
        "Deeply nested File should be imported"
    );
}

#[test]
fn u08_t08_nested_glob_import() {
    let source = r#"
        mod std {
            pub mod io {
                pub mod fs {
                    pub struct File;
                    pub struct Dir;
                }
            }
        }
        use std::io::fs::*;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let file_name = def_map.interner.intern("File");
    let dir_name = def_map.interner.intern("Dir");

    assert!(
        def_map.modules[root_mod].resolve(file_name).is_some(),
        "Nested glob should import File"
    );
    assert!(
        def_map.modules[root_mod].resolve(dir_name).is_some(),
        "Nested glob should import Dir"
    );
}

#[test]
fn u08_t09_multiple_super_levels() {
    let source = r#"
        pub struct Root;
        mod parent {
            pub struct Parent;
            mod child {
                use super::super::Root;
                use super::Parent;
                fn check() {}
            }
        }
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let parent_name = def_map.interner.intern("parent");
    let parent_id = def_map.modules[root_mod]
        .children
        .iter()
        .find(|(n, _)| *n == parent_name)
        .map(|(_, id)| *id)
        .expect("Parent module should exist");

    let child_name = def_map.interner.intern("child");
    let child_id = def_map.modules[parent_id]
        .children
        .iter()
        .find(|(n, _)| *n == child_name)
        .map(|(_, id)| *id)
        .expect("Child module should exist");

    let root_name = def_map.interner.intern("Root");
    let parent_name2 = def_map.interner.intern("Parent");

    assert!(
        def_map.modules[child_id].resolve(root_name).is_some(),
        "Root should be resolvable via super::super"
    );
    assert!(
        def_map.modules[child_id].resolve(parent_name2).is_some(),
        "Parent should be resolvable via super"
    );
}

#[test]
fn u08_t10_module_self_import() {
    let source = r#"
        mod std {
            pub mod io {
                pub struct Read;
            }
        }
        use std::io;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let io_name = def_map.interner.intern("io");

    assert!(
        def_map.modules[root_mod].resolve(io_name).is_some(),
        "Module io should be imported"
    );
}

#[test]
fn u08_t11_mixed_nested_import() {
    let source = r#"
        mod std {
            pub mod io {
                pub struct Read;
                pub struct Write;
            }
            pub mod fs {
                pub struct File;
            }
        }
        use std::io::{Read, Write};
        use std::fs::File;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let read_name = def_map.interner.intern("Read");
    let write_name = def_map.interner.intern("Write");
    let file_name = def_map.interner.intern("File");

    assert!(
        def_map.modules[root_mod].resolve(read_name).is_some(),
        "Read should be imported from nested"
    );
    assert!(
        def_map.modules[root_mod].resolve(write_name).is_some(),
        "Write should be imported from nested"
    );
    assert!(
        def_map.modules[root_mod].resolve(file_name).is_some(),
        "File should be imported from separate path"
    );
}

#[test]
fn u08_t12_glob_with_nested_path() {
    let source = r#"
        mod std {
            pub mod io {
                pub mod net {
                    pub struct TcpStream;
                    pub struct UdpSocket;
                }
            }
        }
        use std::io::net::*;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let tcp_name = def_map.interner.intern("TcpStream");
    let udp_name = def_map.interner.intern("UdpSocket");

    assert!(
        def_map.modules[root_mod].resolve(tcp_name).is_some(),
        "TcpStream should be imported via glob"
    );
    assert!(
        def_map.modules[root_mod].resolve(udp_name).is_some(),
        "UdpSocket should be imported via glob"
    );
}

// Plan §4.1: cross-module glob resolution must work even when the glob source
// module's own re-exports are only finalized after a fixed-point pass. Here `b`
// globs `a`'s public items; `c` globs `b`. The import in `c` only resolves once
// `b`'s glob (pulling `a`'s items) has itself been applied — a single forward
// pass would leave `c`'s glob empty.
#[test]
fn u08_t13_cross_module_glob_fixed_point() {
    let source = r#"
        mod a {
            pub struct Widget;
        }
        mod b {
            pub use crate::a::*;
        }
        mod c {
            pub use crate::b::*;
        }
        use crate::c::Widget;
    "#;
    let (def_map, _) = get_def_map(source);

    let root_mod = def_map.root;
    let widget_name = def_map.interner.intern("Widget");
    assert!(
        def_map.modules[root_mod].resolve(widget_name).is_some(),
        "Widget should resolve in root via chained globs c -> b -> a"
    );
}
