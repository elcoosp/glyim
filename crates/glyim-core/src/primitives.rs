#[derive(Clone, Debug)]
/// TargetInfo.
pub struct TargetInfo {
    pointer_width: u32,
    /// Struct.
    pub triple: String,
    /// Struct.
    pub abi: TargetAbi,
}

impl TargetInfo {
    /// from_triple.
    pub fn from_triple(triple: &str) -> Self {
        let parts: Vec<&str> = triple.split('-').collect();
        let arch = parts.first().copied().unwrap_or("");
        let os = parts.get(2).copied().unwrap_or("");

        let pointer_width = match arch {
            "x86_64" | "aarch64" | "arm64" | "riscv64" | "powerpc64" | "mips64" => 64,
            "i386" | "i686" | "arm" | "armv7" | "wasm32" | "riscv32" | "powerpc" | "mips" => 32,
            _ => {
                // Fallback heuristic for unknown arches
                if arch.contains("64") { 64 } else { 32 }
            }
        };

        let abi = match (arch, os) {
            ("x86_64", "windows") | ("x86_64", "win32") => TargetAbi::X86_64Windows,
            ("aarch64", "windows") | ("arm64", "windows") => TargetAbi::AArch64Windows,
            ("aarch64", _) | ("arm64", _) => TargetAbi::AArch64AAPCS,
            ("wasm32", _) => TargetAbi::Wasm32,
            _ => TargetAbi::X86_64SystemV,
        };

        Self {
            pointer_width,
            triple: triple.to_string(),
            abi,
        }
    }

    /// aarch64.
    pub fn aarch64() -> Self {
        Self::from_triple("aarch64-unknown-linux-gnu")
    }

    /// x86_64.
    pub fn x86_64() -> Self {
        Self::from_triple("x86_64-unknown-linux-gnu")
    }

    /// pointer_width.
    pub fn pointer_width(&self) -> u32 {
        self.pointer_width
    }

    /// pointer_size.
    pub fn pointer_size(&self) -> u64 {
        self.pointer_width as u64 / 8
    }

    /// pointer_align.
    pub fn pointer_align(&self) -> u64 {
        self.pointer_size()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// TargetAbi.
pub enum TargetAbi {
    /// Variant.
    X86_64SystemV,
    /// Variant.
    X86_64Windows,
    /// Variant.
    AArch64AAPCS,
    /// Variant.
    AArch64Windows,
    /// Variant.
    Wasm32,
}

impl Default for TargetInfo {
    fn default() -> Self {
        Self {
            pointer_width: 64,
            triple: "x86_64-unknown-linux-gnu".to_string(),
            abi: TargetAbi::X86_64SystemV,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// IntTy.
pub enum IntTy {
    /// Variant.
    I8,
    /// Variant.
    I16,
    /// Variant.
    I32,
    /// Variant.
    I64,
    /// Variant.
    Isize,
}

impl IntTy {
    /// bit_width.
    pub fn bit_width(self, target: &TargetInfo) -> u32 {
        match self {
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 => 32,
            Self::I64 => 64,
            Self::Isize => target.pointer_width(),
        }
    }
    /// name.
    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Isize => "isize",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// UintTy.
pub enum UintTy {
    /// Variant.
    U8,
    /// Variant.
    U16,
    /// Variant.
    U32,
    /// Variant.
    U64,
    /// Variant.
    Usize,
}

impl UintTy {
    /// bit_width.
    pub fn bit_width(self, target: &TargetInfo) -> u32 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::U32 => 32,
            Self::U64 => 64,
            Self::Usize => target.pointer_width(),
        }
    }
    /// name.
    pub fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::Usize => "usize",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// FloatTy.
pub enum FloatTy {
    /// Variant.
    F32,
    /// Variant.
    F64,
}

impl FloatTy {
    /// bit_width.
    pub fn bit_width(self) -> u32 {
        match self {
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }
    /// name.
    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Mutability.
pub enum Mutability {
    /// Variant.
    Not,
    /// Variant.
    Mut,
}

impl Mutability {
    /// is_mut.
    pub fn is_mut(self) -> bool {
        matches!(self, Self::Mut)
    }
    /// prefix_str.
    pub fn prefix_str(self) -> &'static str {
        if self.is_mut() { "mut " } else { "" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Safety.
pub enum Safety {
    /// Variant.
    Safe,
    /// Variant.
    Unsafe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Abi.
pub enum Abi {
    /// Variant.
    C,
    /// Variant.
    Glyim,
    /// Variant.
    System,
}

impl Abi {
    /// name.
    pub fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::Glyim => "glyim",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// BinOp.
pub enum BinOp {
    /// Variant.
    Add,
    /// Variant.
    Sub,
    /// Variant.
    Mul,
    /// Variant.
    Div,
    /// Variant.
    Rem,
    /// Variant.
    Eq,
    /// Variant.
    Ne,
    /// Variant.
    Lt,
    /// Variant.
    Gt,
    /// Variant.
    LtEq,
    /// Variant.
    GtEq,
    /// Variant.
    And,
    /// Variant.
    Or,
    /// Variant.
    BitAnd,
    /// Variant.
    BitOr,
    /// Variant.
    BitXor,
    /// Variant.
    Shl,
    /// Variant.
    Shr,
}

impl BinOp {
    /// is_comparison.
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::LtEq | Self::GtEq
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// UnOp.
pub enum UnOp {
    /// Variant.
    Not,
    /// Variant.
    Neg,
    /// Variant.
    Deref,
}

use crate::interner::Name;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Visibility.
pub enum Visibility {
    /// Variant.
    Public,
    /// Variant.
    PubCrate,
    /// Variant.
    PubSuper,
    #[allow(missing_docs)]
    PubIn(Vec<Name>),
    /// Variant.
    Inherited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// StructKind.
pub enum StructKind {
    /// Variant.
    Unit,
    /// Variant.
    Tuple,
    /// Variant.
    Record,
}
