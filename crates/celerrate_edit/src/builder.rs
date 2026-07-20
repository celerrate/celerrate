use celerrate_source::{FileId, TextEdit};
use celerrate_syntax::{SyntaxKind, SyntaxToken, lex};

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
}
