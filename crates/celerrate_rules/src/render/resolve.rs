//! Render-time resolution of symbolic labels: a
//! concrete range of another file must never enter a memoized per-file
//! artifact, so the stored form is the declaration's display path and
//! THIS module turns it into a location, at render time, outside
//! queries.
//!
//! Current scope: bare class-like and function symbols resolve; member
//! symbols (`Class::member`), stub-backed, define-origin, and unknown
//! symbols degrade to the note form. Member precision arrives with the
//! first rule that emits a member label.

use celerrate_db::AnalyzedFileSet;
use celerrate_semantics::{
    AstId, SymbolQuery, SymbolSpace, analyzed_file_index, ast_id_map, folded_symbol_key,
    lookup_class_declaration, lookup_function_declaration,
};
use celerrate_source::{FileId, TextRange, TextSize};

use super::{ResolvedLabel, SymbolResolver};

/// The database-backed resolver the CLI wires at the composition root.
pub struct DatabaseResolver<'db> {
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
}

impl<'db> DatabaseResolver<'db> {
    pub fn new(db: &'db dyn salsa::Database, files: AnalyzedFileSet) -> Self {
        Self { db, files }
    }

    fn declaration_of(&self, symbol: &str) -> Option<AstId> {
        let class_query = SymbolQuery::new(
            self.db,
            SymbolSpace::ClassLike,
            folded_symbol_key(SymbolSpace::ClassLike, symbol),
        );
        if let Some((_, ast_id)) = lookup_class_declaration(self.db, self.files, class_query) {
            return Some(ast_id);
        }
        let function_query = SymbolQuery::new(
            self.db,
            SymbolSpace::Function,
            folded_symbol_key(SymbolSpace::Function, symbol),
        );
        lookup_function_declaration(self.db, self.files, function_query)
    }

    fn first_line_of(&self, ast_id: AstId) -> Option<(FileId, TextRange)> {
        let index = analyzed_file_index(self.db, self.files);
        let (_, source_file) = index.iter().find(|(file, _)| *file == ast_id.file)?;
        let map = ast_id_map(self.db, *source_file);
        let pointer = map.pointer(ast_id.index)?;
        let root = celerrate_db::parse(self.db, *source_file).tree();
        let node_range = pointer.try_to_node(&root)?.text_range();
        let line_index = celerrate_db::line_index(self.db, *source_file);
        let line = line_index.line_column(node_range.start()).line;
        let line_range = line_index.line_range(line)?;
        let text = celerrate_db::source_text(self.db, *source_file)
            .as_ref()
            .ok()?
            .text();
        Some((ast_id.file, clip_to_line(node_range, line_range, text)))
    }
}

impl SymbolResolver for DatabaseResolver<'_> {
    fn resolve(&self, symbol: &str) -> ResolvedLabel {
        if symbol.contains("::") {
            return ResolvedLabel::Degraded;
        }
        match self
            .declaration_of(symbol)
            .and_then(|ast_id| self.first_line_of(ast_id))
        {
            Some((file, range)) => ResolvedLabel::Concrete { file, range },
            None => ResolvedLabel::Degraded,
        }
    }
}

/// A whole-declaration underline would span the class body; the label
/// points at the declaration, so its first line carries the meaning.
fn clip_to_line(node: TextRange, line: TextRange, text: &str) -> TextRange {
    let end = node.end().min(line.end());
    if end <= node.start() {
        return node;
    }
    let start_usize = u32::from(node.start()) as usize;
    let end_usize = u32::from(end) as usize;
    let Some(slice) = text.get(start_usize..end_usize) else {
        return node;
    };
    let trimmed = slice.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return node;
    }
    TextRange::new(node.start(), node.start() + TextSize::of(trimmed))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_source::FileId;

    use super::DatabaseResolver;
    use crate::render::{ResolvedLabel, SymbolResolver};

    const DECLARING: &str =
        "<?php\nnamespace App;\n\nclass User\n{\n    public function save(): void {}\n}\n";

    fn fixture() -> (TestDatabase, AnalyzedFileSet) {
        let db = TestDatabase::default();
        let declaring = SourceFile::new(&db, FileId::new(0), DECLARING.as_bytes().to_vec());
        let files = AnalyzedFileSet::new(&db, vec![declaring]);
        (db, files)
    }

    #[test]
    fn a_source_class_resolves_to_the_first_line_of_its_declaration() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        match resolver.resolve("App\\User") {
            ResolvedLabel::Concrete { file, range } => {
                assert_eq!(file, FileId::new(0));
                let start = u32::from(range.start()) as usize;
                let end = u32::from(range.end()) as usize;
                assert_eq!(&DECLARING[start..end], "class User");
            }
            ResolvedLabel::Degraded => panic!("a source class must resolve"),
        }
    }

    #[test]
    fn a_member_symbol_degrades_in_this_sub_project() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        assert!(matches!(
            resolver.resolve("App\\User::save"),
            ResolvedLabel::Degraded
        ));
    }

    #[test]
    fn an_unknown_symbol_degrades() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        assert!(matches!(
            resolver.resolve("App\\Missing"),
            ResolvedLabel::Degraded
        ));
    }
}
