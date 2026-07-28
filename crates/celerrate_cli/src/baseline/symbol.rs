//! The enclosing symbol path of a finding: the locality a line number
//! used to provide, without its fragility. Read-only over existing
//! semantic queries; nothing here is a query itself.

use celerrate_db::AnalyzedFileSet;
use celerrate_semantics::{analyzed_file_index, ast_id_map, fully_qualified_name, member_tree};
use celerrate_source::{FileId, TextRange};

/// The marker for a finding that falls outside every declaration.
pub const TOP_LEVEL_SYMBOL: &str = "(top level)";
/// The marker for the class part of an anonymous class-like's path.
pub const ANONYMOUS_CLASS: &str = "(anonymous class)";

/// The innermost declaration whose syntax range contains `range.start()`:
/// `<class display>::<member name>` for a member, the fully qualified name
/// for a bare class-like or a free function, and [`TOP_LEVEL_SYMBOL`] for
/// anything else (including inside a closure, which is not a declaration).
/// A file this database cannot resolve yields [`TOP_LEVEL_SYMBOL`]: no
/// input may ever crash the tool.
pub fn enclosing_symbol_path(
    database: &dyn salsa::Database,
    files: AnalyzedFileSet,
    file: FileId,
    range: TextRange,
) -> String {
    let Some(source_file) = source_file_of(database, files, file) else {
        return TOP_LEVEL_SYMBOL.to_string();
    };
    let root = celerrate_db::parse(database, source_file).tree();
    let ast_ids = ast_id_map(database, source_file);
    let members = member_tree(database, source_file);

    let mut best: Option<(TextRange, String)> = None;
    // A closure rather than a free function: naming the node type it
    // walks would require depending on `celerrate_syntax` outside tests,
    // where today it is only a dev-dependency: this call chain never
    // needs the type spelled out.
    let mut consider = |index: u32, display: String| {
        let Some(pointer) = ast_ids.pointer(index) else {
            return;
        };
        let Some(node) = pointer.try_to_node(&root) else {
            return;
        };
        let node_range = node.text_range();
        if !node_range.contains(range.start()) {
            return;
        }
        let smaller = best
            .as_ref()
            .is_none_or(|(current, _)| node_range.len() < current.len());
        if smaller {
            best = Some((node_range, display));
        }
    };

    for class in &members.classes {
        let class_display = match &class.name {
            Some(name) => fully_qualified_name(&class.namespace, name),
            None => ANONYMOUS_CLASS.to_string(),
        };
        consider(class.ast_id.index, class_display.clone());
        for member in &class.members {
            consider(
                member.ast_id.index,
                format!("{class_display}::{}", member.name),
            );
        }
    }
    for function in &members.functions {
        consider(
            function.ast_id.index,
            fully_qualified_name(&function.namespace, &function.name),
        );
    }
    best.map_or_else(|| TOP_LEVEL_SYMBOL.to_string(), |(_, display)| display)
}

/// The `SourceFile` handle of `file` within `files`, when it is one of the
/// analyzed files: the bridge from the caller's `FileId` to the salsa
/// handle the semantic queries key on.
fn source_file_of(
    database: &dyn salsa::Database,
    files: AnalyzedFileSet,
    file: FileId,
) -> Option<celerrate_db::SourceFile> {
    let index = analyzed_file_index(database, files);
    index
        .iter()
        .find(|(candidate, _)| *candidate == file)
        .map(|(_, source_file)| *source_file)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::session::Session;

    const MANIFEST: &str =
        r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

    /// Builds a session over one PHP file and returns the symbol path at the
    /// first occurrence of `needle` in that file.
    fn symbol_at(source: &str, needle: &str) -> String {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("composer.json"), MANIFEST).unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let file_path = root.path().join("src").join("Example.php");
        std::fs::write(&file_path, source).unwrap();
        let mut session = Session::start(root.path());
        let file = session.vfs.file_id(&file_path);
        let offset = u32::try_from(source.find(needle).unwrap()).unwrap();
        let range = celerrate_source::TextRange::new(
            offset.into(),
            (offset + u32::try_from(needle.len()).unwrap()).into(),
        );
        enclosing_symbol_path(&session.database, session.files, file, range)
    }

    #[test]
    fn a_finding_in_a_method_keys_on_class_and_method() {
        let source = "<?php\nnamespace App\\Service;\n\nclass Checkout\n{\n    public function finalize(): void\n    {\n        new Missing();\n    }\n}\n";
        assert_eq!(
            symbol_at(source, "new Missing"),
            "App\\Service\\Checkout::finalize"
        );
    }

    #[test]
    fn a_finding_in_a_free_function_keys_on_the_function() {
        let source = "<?php\nnamespace App;\n\nfunction helper(): void\n{\n    new Missing();\n}\n";
        assert_eq!(symbol_at(source, "new Missing"), "App\\helper");
    }

    #[test]
    fn a_finding_on_a_class_header_keys_on_the_class() {
        let source = "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";
        assert_eq!(symbol_at(source, "Missing"), "App\\Kernel");
    }

    #[test]
    fn a_finding_outside_declarations_is_top_level() {
        let source = "<?php\n\nnew Missing();\n";
        assert_eq!(symbol_at(source, "new Missing"), TOP_LEVEL_SYMBOL);
    }

    #[test]
    fn a_finding_in_a_closure_keys_on_the_enclosing_method() {
        let source = "<?php\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        $callback = function (): void {\n            new Missing();\n        };\n    }\n}\n";
        assert_eq!(symbol_at(source, "new Missing"), "App\\Runner::run");
    }

    #[test]
    fn an_anonymous_class_method_uses_the_anonymous_marker() {
        let source = "<?php\n\n$instance = new class {\n    public function run(): void\n    {\n        new Missing();\n    }\n};\n";
        assert_eq!(
            symbol_at(source, "new Missing"),
            format!("{ANONYMOUS_CLASS}::run")
        );
    }
}
