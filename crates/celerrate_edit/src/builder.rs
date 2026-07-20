use celerrate_source::{FileId, TextEdit, TextRange};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, lex};

use crate::conflict::{EditConflict, find_conflict};

/// Why a structured operation could not be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The replacement text does not lex to exactly one clean token.
    ReplacementIsNotOneToken { replacement: String },
    /// The replacement lexes to a token of a different kind than the
    /// one it replaces, so the edit would change structure, not text.
    ReplacementChangesKind {
        expected: SyntaxKind,
        actual: SyntaxKind,
        replacement: String,
    },
    /// The comment text would terminate the comment early: it contains
    /// a line break or a PHP close tag.
    CommentTextBreaksOut { text: String },
}

/// Records structured operations against one file's syntax tree and
/// compiles them into the deterministic, sorted, conflict-free
/// [`TextEdit`] set. An operation never touches trivia it was not
/// aimed at.
pub struct EditBuilder {
    file: FileId,
    edits: Vec<TextEdit>,
}

impl EditBuilder {
    pub fn new(file: FileId) -> Self {
        Self {
            file,
            edits: Vec::new(),
        }
    }

    /// Replaces one token's text, keeping its kind: the replacement
    /// must lex to exactly one clean token of the same kind, so a
    /// rename can never smuggle structure through an edit.
    pub fn replace_token(
        &mut self,
        token: &SyntaxToken,
        replacement: &str,
    ) -> Result<(), EditError> {
        let actual = single_token_kind(replacement)?;
        if actual != token.kind() {
            return Err(EditError::ReplacementChangesKind {
                expected: token.kind(),
                actual,
                replacement: replacement.to_owned(),
            });
        }
        self.edits.push(TextEdit {
            file: self.file,
            range: token.text_range(),
            replacement: replacement.to_owned(),
        });
        Ok(())
    }

    /// Inserts `// text` on its own line directly above `node`,
    /// reproducing the node's indentation. The edit is a pure insertion
    /// at the node's first byte, so the trivia already in front of the
    /// node are never touched.
    pub fn insert_line_comment_before(
        &mut self,
        node: &SyntaxNode,
        text: &str,
    ) -> Result<(), EditError> {
        if text.contains('\n') || text.contains('\r') || text.contains("?>") {
            return Err(EditError::CommentTextBreaksOut {
                text: text.to_owned(),
            });
        }
        let indentation = indentation_before(node);
        self.edits.push(TextEdit {
            file: self.file,
            range: TextRange::empty(node.text_range().start()),
            replacement: format!("// {text}\n{indentation}"),
        });
        Ok(())
    }

    /// Finalizes into the sorted edit set, or reports the first
    /// conflict. The set is the terminal, tree-free form suggestions
    /// transport and [`crate::apply`] consumes.
    pub fn finish(self) -> Result<Vec<TextEdit>, EditConflict> {
        let mut edits = self.edits;
        edits.sort();
        match find_conflict(&edits) {
            Some(conflict) => Err(conflict),
            None => Ok(edits),
        }
    }
}

/// Lexes `replacement` in scripting mode (behind a synthetic `<?php `
/// prefix, because the lexer starts in inline-HTML mode) and returns
/// its kind when it is exactly one clean token: no lexer diagnostics,
/// one non-trivia token past the open tag, and nothing else — the
/// length comparison rejects trailing trivia.
fn single_token_kind(replacement: &str) -> Result<SyntaxKind, EditError> {
    let not_one_token = || EditError::ReplacementIsNotOneToken {
        replacement: replacement.to_owned(),
    };
    let (tokens, diagnostics) = lex(&format!("<?php {replacement}"));
    if !diagnostics.is_empty() {
        return Err(not_one_token());
    }
    let mut meaningful = tokens
        .iter()
        .skip(1) // the synthetic open tag
        .filter(|token| !token.kind.is_trivia());
    let Some(first) = meaningful.next() else {
        return Err(not_one_token());
    };
    if meaningful.next().is_some() {
        return Err(not_one_token());
    }
    if usize::from(first.length) != replacement.len() {
        return Err(not_one_token());
    }
    Ok(first.kind)
}

/// The whitespace run between the last line break and `node`, used to
/// reproduce the node's indentation on an inserted line. A node with
/// no preceding whitespace token has no indentation to reproduce; a
/// mid-line node (preceding whitespace without a line break) reuses
/// that whitespace as-is.
fn indentation_before(node: &SyntaxNode) -> String {
    let Some(first_token) = node.first_token() else {
        return String::new();
    };
    let Some(previous) = first_token.prev_token() else {
        return String::new();
    };
    if previous.kind() != SyntaxKind::Whitespace {
        return String::new();
    }
    let text = previous.text();
    let after_break = text.rfind('\n').map_or(0, |index| index + 1);
    text.get(after_break..).unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;
    use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

    use super::{EditBuilder, EditError};
    use crate::apply;

    fn parse_tree(source: &str) -> SyntaxNode {
        celerrate_syntax::parse(source).tree()
    }

    fn token_with_text(root: &SyntaxNode, text: &str) -> SyntaxToken {
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.text() == text)
            .expect("the token under edit exists in the tree")
    }

    #[test]
    fn a_token_is_replaced_in_place() {
        let source = "<?php strlen($value);";
        let root = parse_tree(source);
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php strrev($value);");
    }

    #[test]
    fn a_replacement_of_a_different_length_still_lands_exactly() {
        let source = "<?php userNme();";
        let root = parse_tree(source);
        let token = token_with_text(&root, "userNme");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "userName").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php userName();");
    }

    #[test]
    fn surrounding_trivia_are_never_touched() {
        let source = "<?php  /* keep */  strlen($value);";
        let root = parse_tree(source);
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php  /* keep */  strrev($value);",
        );
    }

    #[test]
    fn a_replacement_that_is_two_tokens_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, "foo bar"),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: "foo bar".to_owned(),
            }),
        );
    }

    #[test]
    fn a_replacement_with_trailing_whitespace_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, "foo "),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: "foo ".to_owned(),
            }),
        );
    }

    #[test]
    fn an_empty_replacement_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, ""),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: String::new(),
            }),
        );
    }

    #[test]
    fn a_replacement_with_a_lexer_error_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert!(matches!(
            builder.replace_token(&token, "\"unterminated"),
            Err(EditError::ReplacementIsNotOneToken { .. }),
        ));
    }

    #[test]
    fn renaming_an_identifier_to_a_keyword_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        let error = builder.replace_token(&token, "class").unwrap_err();
        assert!(matches!(
            error,
            EditError::ReplacementChangesKind {
                expected: SyntaxKind::Identifier,
                ..
            },
        ));
    }

    #[test]
    fn finish_sorts_the_edits_into_the_total_order() {
        let source = "<?php first(); second();";
        let root = parse_tree(source);
        let second = token_with_text(&root, "second");
        let first = token_with_text(&root, "first");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&second, "later").unwrap();
        builder.replace_token(&first, "early").unwrap();
        let edits = builder.finish().unwrap();
        assert!(edits[0].range.start() < edits[1].range.start());
        assert_eq!(apply(source, &edits).unwrap(), "<?php early(); later();");
    }

    #[test]
    fn replacing_the_same_token_twice_is_a_conflict() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        builder.replace_token(&token, "strtolower").unwrap();
        assert!(builder.finish().is_err());
    }

    fn first_node_of_kind(root: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
        root.descendants()
            .find(|node| node.kind() == kind)
            .expect("the target node exists in the tree")
    }

    #[test]
    fn a_comment_is_inserted_above_an_indented_statement() {
        let source = "<?php\nfunction demo() {\n    echo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder
            .insert_line_comment_before(&statement, "@celerrate-ignore CEL0018")
            .unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\nfunction demo() {\n    // @celerrate-ignore CEL0018\n    echo 1;\n}\n",
        );
    }

    #[test]
    fn a_comment_above_a_top_level_statement_carries_no_indentation() {
        let source = "<?php\necho 1;\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder
            .insert_line_comment_before(&statement, "note")
            .unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php\n// note\necho 1;\n");
    }

    #[test]
    fn tab_indentation_is_reproduced() {
        let source = "<?php\nfunction demo() {\n\techo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder
            .insert_line_comment_before(&statement, "note")
            .unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\nfunction demo() {\n\t// note\n\techo 1;\n}\n",
        );
    }

    #[test]
    fn a_mid_line_node_stays_intact_after_insertion() {
        // The comment ends with a line break before the node, so the
        // statement survives even when the node is not at a line start.
        let source = "<?php\necho 1; echo 2;\n";
        let root = parse_tree(source);
        let second = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::EchoStatement)
            .nth(1)
            .expect("the second echo statement");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.insert_line_comment_before(&second, "note").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\necho 1; // note\n echo 2;\n",
        );
    }

    #[test]
    fn comment_text_with_a_line_break_is_rejected() {
        let root = parse_tree("<?php\necho 1;\n");
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.insert_line_comment_before(&statement, "a\nb"),
            Err(EditError::CommentTextBreaksOut {
                text: "a\nb".to_owned(),
            }),
        );
    }

    #[test]
    fn comment_text_with_a_close_tag_is_rejected() {
        let root = parse_tree("<?php\necho 1;\n");
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.insert_line_comment_before(&statement, "a ?> b"),
            Err(EditError::CommentTextBreaksOut {
                text: "a ?> b".to_owned(),
            }),
        );
    }

    #[test]
    fn an_inserted_comment_reparses_as_a_comment() {
        // The guarantee behind the validation: the patched file lexes
        // with the inserted text inside a line comment, not as code.
        let source = "<?php\nfunction demo() {\n    echo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder
            .insert_line_comment_before(&statement, "note")
            .unwrap();
        let edits = builder.finish().unwrap();
        let patched = apply(source, &edits).unwrap();
        let reparsed = parse_tree(&patched);
        let comment = reparsed
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::LineComment)
            .expect("the inserted comment lexes as a line comment");
        assert_eq!(comment.text(), "// note");
    }
}
