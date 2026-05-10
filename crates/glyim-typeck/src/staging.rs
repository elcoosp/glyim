/// The stage at which an expression is evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Comptime = 0,
    Buildtime = 1,
    Runtime = 2,
}
