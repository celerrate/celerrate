/// An opaque, compact handle identifying one source file.
///
/// The crate attaches no meaning to the value: identifiers are assigned by
/// the layer that discovers files (the query database, in a later
/// sub-project) and serve as cheap `Copy` keys everywhere below it. The
/// mapping between identifiers and paths lives with the assigner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// Wraps a raw identifier assigned by the caller.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw identifier, for the layer that assigned it.
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}
