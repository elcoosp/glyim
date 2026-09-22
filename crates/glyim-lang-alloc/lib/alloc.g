//! Global allocator API and memory layout for the Glyim alloc library.
//!
//! The global allocator is wired to the runtime's C-FFI allocation functions
//! (`glyim_alloc` / `glyim_dealloc` / `glyim_process_abort`, defined in
//! `glyim-runtime`). `Box`, `Vec` and `String` lower to calls through the
//! `GLOBAL` allocator below, so without this wiring no user program can
//! allocate heap memory.

/// A `Layout` describes a particular block of memory.
struct Layout {
    size: usize,
    align: usize,
}

impl Layout {
    /// Creates a new `Layout` from the given `size` and `align`.
    fn from_size_align(size: usize, align: usize) -> Result<Layout, LayoutError> {
        if !align.is_power_of_two() {
            return Result::Err(LayoutError::InvalidAlignment(align));
        }
        if size > usize::MAX - (align - 1) {
            return Result::Err(LayoutError::SizeOverflow);
        }
        Result::Ok(Layout { size, align })
    }

    /// The minimum size in bytes for a memory block of this layout.
    fn size(&self) -> usize {
        self.size
    }

    /// The minimum alignment for a memory block of this layout.
    fn align(&self) -> usize {
        self.align
    }
}

/// Error returned by `Layout::from_size_align` when parameters are invalid.
enum LayoutError {
    /// The alignment was not a power of two.
    InvalidAlignment(usize),
    /// The requested size overflows the address space.
    SizeOverflow,
}

/// Trait for custom global allocators.
trait GlobalAlloc {
    /// Allocate memory as described by `layout`.
    fn alloc(&self, layout: Layout) -> *mut u8;

    /// Deallocate the memory referenced by `ptr`.
    fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

/// Aborts the process when an allocation cannot be satisfied, mirroring the
/// `handle_alloc_error` contract expected by `Box`/`Vec`/`String`.
fn handle_alloc_error(_layout: Layout) -> ! {
    extern "C" {
        fn glyim_process_abort() -> !;
    }
    unsafe { glyim_process_abort() }
}

/// The global allocator, backed by the runtime's C-FFI allocation fns.
/// A zero-sized unit struct: every method call borrows `self` and forwards
/// to the runtime, so `GLOBAL.alloc(layout)` / `GLOBAL.dealloc(ptr, layout)`
/// are valid from `boxed.g` / `rc.g` / `raw_vec.g`.
const GLOBAL: Global = Global;

struct Global;

impl GlobalAlloc for Global {
    fn alloc(&self, layout: Layout) -> *mut u8 {
        extern "C" {
            fn glyim_alloc(size: usize, align: usize) -> *mut u8;
        }
        unsafe { glyim_alloc(layout.size, layout.align) }
    }

    fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        extern "C" {
            fn glyim_dealloc(ptr: *mut u8, size: usize, align: usize);
        }
        unsafe { glyim_dealloc(ptr, layout.size, layout.align) }
    }
}
