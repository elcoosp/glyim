use crate::dispatch::DispatchTable;
use glyim_interner::Interner;

#[test]
fn dispatch_table_new_is_empty() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("main");
    assert_eq!(table.get_address(name), 0);
}

#[test]
fn dispatch_table_insert_and_get() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("add");
    table.update(name, 0xDEADBEEF);
    assert_eq!(table.get_address(name), 0xDEADBEEF);
}

#[test]
fn dispatch_table_update_replaces_address() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("foo");
    table.update(name, 0x1000);
    table.update(name, 0x2000);
    assert_eq!(table.get_address(name), 0x2000);
}

#[test]
fn dispatch_table_multiple_functions() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let add = interner.intern("add");
    let sub = interner.intern("sub");
    table.update(add, 0x1000);
    table.update(sub, 0x2000);
    assert_eq!(table.get_address(add), 0x1000);
    assert_eq!(table.get_address(sub), 0x2000);
}

#[test]
fn dispatch_table_unknown_function_returns_zero() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let unknown = interner.intern("unknown");
    assert_eq!(table.get_address(unknown), 0);
}

#[test]
fn dispatch_table_contains() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("present");
    assert!(!table.contains(name));
    table.update(name, 0x42);
    assert!(table.contains(name));
}

#[test]
fn dispatch_table_remove() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("temp");
    table.update(name, 0x42);
    assert!(table.contains(name));
    table.remove(name);
    assert!(!table.contains(name));
    assert_eq!(table.get_address(name), 0);
}

#[test]
fn dispatch_table_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<DispatchTable>();
}

#[test]
fn dispatch_table_concurrent_updates() {
    use std::sync::Arc;
    use std::thread;

    let table = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let name = interner.intern("concurrent");

    let table1 = Arc::clone(&table);
    let table2 = Arc::clone(&table);

    let h1 = thread::spawn(move || {
        for i in 0..100 {
            table1.update(name, 0x1000 + i);
        }
    });
    let h2 = thread::spawn(move || {
        for i in 0..100 {
            table2.update(name, 0x2000 + i);
        }
    });

    h1.join().unwrap();
    h2.join().unwrap();

    let addr = table.get_address(name);
    assert!(addr >= 0x1000);
}

#[test]
fn dispatch_table_len() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    assert_eq!(table.len(), 0);
    table.update(interner.intern("a"), 1);
    table.update(interner.intern("b"), 2);
    assert_eq!(table.len(), 2);
}
