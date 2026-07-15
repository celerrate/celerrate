//! The pinned phpstan/phpdoc-parser reference: fetches the snapshot
//! at its pin and extracts the `TypeParserTest::provideParseData`
//! inputs into the committed case file the bridge's coverage test
//! consumes. The extractor is a string-aware bracket scanner over the
//! provider region — layout-coupled to the pinned commit by design,
//! guarded by `--check` in CI.

use std::path::PathBuf;

use crate::{Result, pin, workspace_root};

const PIN_FILE: &str = "phpdoc-parser.pin";
const SOURCE_FILE: &str = "tests/PHPStan/Parser/TypeParserTest.php";
const CASES_FILE: &str = "crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/cases.txt";

pub fn fetch() -> Result<PathBuf> {
    let root = workspace_root()?;
    let pin = pin::read(&root.join("xtask").join(PIN_FILE))?;
    let directory = root.join("target").join("phpdoc-parser").join(&pin.commit);
    pin::fetch_snapshot(&pin, &directory)?;
    Ok(directory)
}

pub fn extract(check: bool) -> Result<()> {
    let root = workspace_root()?;
    let pin = pin::read(&root.join("xtask").join(PIN_FILE))?;
    let snapshot = fetch()?;
    let source = std::fs::read_to_string(snapshot.join(SOURCE_FILE))?;
    let cases = extract_cases(&source)?;
    let mut rendered = String::new();
    rendered.push_str(
        "# Type-expression inputs extracted from tests/PHPStan/Parser/TypeParserTest.php\n",
    );
    rendered.push_str("# (provideParseData). One case per line; \\ \\n \\r \\t escaped.\n");
    rendered.push_str(&format!("# repository = {}\n", pin.repository));
    rendered.push_str(&format!("# commit = {}\n", pin.commit));
    rendered.push_str("# license = MIT (the upstream LICENSE covers the extracted inputs)\n");
    rendered.push_str(&format!("# cases = {}\n", cases.len()));
    for case in &cases {
        rendered.push_str(&escape(case));
        rendered.push('\n');
    }
    let destination = root.join(CASES_FILE);
    if check {
        let committed = std::fs::read_to_string(&destination)?;
        if committed != rendered {
            return Err(
                "the committed phpdoc corpus cases are stale: run `cargo xtask phpdoc-cases`"
                    .into(),
            );
        }
        println!("phpdoc corpus cases are current ({} cases)", cases.len());
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&destination, rendered)?;
        println!("wrote {} cases to {}", cases.len(), destination.display());
    }
    Ok(())
}

/// The provider region runs from the `provideParseData` header
/// through its `return [` to the bracket that closes it. Each case
/// opens one bracket level below the return array; its input is the
/// PHP string literal (or `.`-concatenated chain of them, see
/// `read_case_input`) that follows the opener. Depth counting is
/// string-aware, so brackets inside inputs do not derail it.
fn extract_cases(source: &str) -> Result<Vec<String>> {
    let start = source
        .find("public function provideParseData(): array")
        .ok_or("provideParseData not found: the pinned layout changed")?;
    let region = source.get(start..).ok_or("provider region out of range")?;
    let open = region
        .find("return [")
        .ok_or("provider return not found: the pinned layout changed")?
        + "return [".len();
    let mut characters = region
        .get(open..)
        .ok_or("provider body out of range")?
        .chars()
        .peekable();
    let mut depth: u32 = 1;
    let mut cases = Vec::new();
    while let Some(character) = characters.next() {
        match character {
            '\'' | '"' => {
                let _ = read_php_string(&mut characters, character);
            }
            '[' => {
                depth += 1;
                if depth == 2 {
                    skip_whitespace(&mut characters);
                    if let Some(case) = read_case_input(&mut characters) {
                        cases.push(case);
                    }
                }
            }
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    if cases.is_empty() {
        return Err("no cases extracted: the pinned layout changed".into());
    }
    Ok(cases)
}

fn skip_whitespace(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(next) = characters.peek() {
        if next.is_whitespace() {
            characters.next();
        } else {
            break;
        }
    }
}

/// A case's input is a PHP string literal, or several joined by `.`
/// (the pinned reference wraps its multiline generic and callable
/// inputs this way, one physical line per literal, for readability).
/// `PHP_EOL` is the only bare identifier a concatenation segment may
/// be: it renders `\n`, matching every other multiline case's literal
/// `\n`. `None` if the position holds no case (a comment, say) or a
/// concatenation segment this does not recognize.
fn read_case_input(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let &quote = characters.peek()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    characters.next();
    let mut value = read_php_string(characters, quote)?;
    loop {
        skip_whitespace(characters);
        if characters.peek() != Some(&'.') {
            return Some(value);
        }
        characters.next();
        skip_whitespace(characters);
        match characters.peek().copied() {
            Some(quote @ ('\'' | '"')) => {
                characters.next();
                value.push_str(&read_php_string(characters, quote)?);
            }
            Some(_) => {
                let mut identifier = String::new();
                while let Some(&character) = characters.peek() {
                    if character.is_alphanumeric() || character == '_' {
                        identifier.push(character);
                        characters.next();
                    } else {
                        break;
                    }
                }
                if identifier != "PHP_EOL" {
                    return None;
                }
                value.push('\n');
            }
            None => return None,
        }
    }
}

/// PHP string semantics per quote kind: single quotes decode `\\` and
/// `\'` and keep any other escape verbatim; double quotes additionally
/// decode `\"`, `\n`, `\t`, `\r`. `None` on an unterminated literal.
fn read_php_string(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    quote: char,
) -> Option<String> {
    let mut value = String::new();
    while let Some(character) = characters.next() {
        if character == quote {
            return Some(value);
        }
        if character == '\\' {
            match characters.next()? {
                '\\' => value.push('\\'),
                escaped if escaped == quote => value.push(quote),
                'n' if quote == '"' => value.push('\n'),
                't' if quote == '"' => value.push('\t'),
                'r' if quote == '"' => value.push('\r'),
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        } else {
            value.push(character);
        }
    }
    None
}

fn escape(case: &str) -> String {
    case.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{escape, extract_cases};

    const MINIATURE: &str = r#"
class TypeParserTest extends TestCase
{
    public function provideParseData(): array
    {
        return [
            [
                'string',
                new IdentifierTypeNode('string'),
            ],
            [
                'array{
                    // a is for [apple]
                    a: int,
                }',
                ArrayShapeNode::createSealed([
                    new ArrayShapeItemNode(null, false, new IdentifierTypeNode('int')),
                ]),
            ],
            [
                'it\'s',
                new ConstTypeNode(new ConstExprStringNode("it's", 1)),
            ],
            [
                'array<' . PHP_EOL .
                '  Foo' . PHP_EOL .
                '>',
                new GenericTypeNode(new IdentifierTypeNode('array'), [
                    new IdentifierTypeNode('Foo'),
                ]),
            ],
        ];
    }

    public function unrelated(): array
    {
        return [['not a case']];
    }
}
"#;

    #[test]
    fn cases_extract_from_the_provider_region_only() {
        let cases = extract_cases(MINIATURE).unwrap();
        assert_eq!(cases.len(), 4);
        assert_eq!(cases[0], "string");
        // Multiline inputs survive whole, brackets inside strings do
        // not derail the depth scan.
        assert!(cases[1].contains("// a is for [apple]"));
        // PHP single-quote escapes decode.
        assert_eq!(cases[2], "it's");
    }

    #[test]
    fn a_dot_concatenated_input_joins_with_php_eol_as_a_newline() {
        let cases = extract_cases(MINIATURE).unwrap();
        // The pinned reference wraps several multiline generic and
        // callable inputs as `'...' . PHP_EOL . '...'` chains, for
        // readability rather than embedded `\n`. Each segment joins
        // in order, `PHP_EOL` rendering as a single `\n`.
        assert_eq!(cases[3], "array<\n  Foo\n>");
    }

    #[test]
    fn escaping_is_line_safe() {
        assert_eq!(escape("a\nb\\c"), "a\\nb\\\\c");
    }
}
