//! The docblock lexer: one per plugin, shared by every dialect
//! module. Total over arbitrary input (fuzzed): any string yields a
//! tag list, never a panic.

/// One `@tag` occurrence: the name without `@`, and the raw content
/// up to the next tag or the end of the docblock, decoration
/// stripped, continuation lines folded with single spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub content: String,
}

/// Splits a docblock into its tags. Summary prose before the first
/// tag is ignored; inline `{@...}` forms are not interpreted.
#[allow(clippy::collapsible_if)]
pub fn lex_docblock(text: &str) -> Vec<Tag> {
    let body = text.strip_prefix("/**").unwrap_or(text);
    let body = body.strip_suffix("*/").unwrap_or(body);
    let mut tags: Vec<Tag> = Vec::new();
    for line in body.lines() {
        let line = line.trim_start().trim_start_matches('*').trim();
        if let Some(rest) = line.strip_prefix('@') {
            let boundary = rest
                .find(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
                .unwrap_or(rest.len());
            let name = rest.get(..boundary).unwrap_or_default();
            let content = rest.get(boundary..).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            tags.push(Tag {
                name: name.to_owned(),
                content: content.trim().to_owned(),
            });
        } else if !line.is_empty() {
            if let Some(open) = tags.last_mut() {
                if !open.content.is_empty() {
                    open.content.push(' ');
                }
                open.content.push_str(line);
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::indexing_slicing)]
    #[test]
    fn tags_split_on_lines_with_decoration_stripped() {
        let tags = lex_docblock(
            "/**\n * Summary prose, ignored.\n *\n * @param int $id the identifier\n * @return string\n */",
        );
        assert_eq!(
            tags,
            vec![
                Tag {
                    name: "param".to_owned(),
                    content: "int $id the identifier".to_owned()
                },
                Tag {
                    name: "return".to_owned(),
                    content: "string".to_owned()
                },
            ],
        );
    }

    #[allow(clippy::indexing_slicing)]
    #[test]
    fn a_single_line_docblock_lexes() {
        assert_eq!(
            lex_docblock("/** @return int */"),
            vec![Tag {
                name: "return".to_owned(),
                content: "int".to_owned()
            }],
        );
    }

    #[allow(clippy::indexing_slicing)]
    #[test]
    fn continuation_lines_fold_into_the_open_tag() {
        let tags = lex_docblock("/**\n * @param int $id\n *   spanning two lines\n */");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].content, "int $id spanning two lines");
    }

    #[allow(clippy::indexing_slicing)]
    #[test]
    fn hyphenated_tag_names_lex_whole() {
        let tags = lex_docblock("/** @property-read string $title */");
        assert_eq!(tags[0].name, "property-read");
        assert_eq!(tags[0].content, "string $title");
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        for input in [
            "",
            "/**",
            "*/",
            "/**/",
            "@",
            "/** @ */",
            "/** @@ */",
            "/** *** */",
            "no docblock at all",
            "/** @return */",
            "\u{0}\u{0}@\u{0}",
            "/**\r\n * @var\r\n */",
        ] {
            let _ = lex_docblock(input);
        }
    }
}
