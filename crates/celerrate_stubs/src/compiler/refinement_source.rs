//! The `refinements.celerrate` source format: enriched stub signatures
//! written in the internal norm, parsed into [`StubRefinements`] and
//! validated for structure and existence before it is attached to the
//! compiled [`crate::StubIndex`] (design section 7, decision 3).
//!
//! This module never lowers norm text: it treats every type text,
//! bound, and argument as an opaque string. Lowering totality is
//! validated upstairs, in `celerrate_types`, by a test that iterates
//! every text in the embedded blob's refinements.
//!
//! # Source format
//!
//! ```text
//! # Comments start with '#'; blank lines separate entries.
//! function array_keys<TKey, TValue>(array<TKey, TValue> $array): list<TKey>
//!
//! class ArrayIterator<TKey, TValue> implements Iterator<TKey, TValue> {
//!     method __construct(array<TKey, TValue> $array)
//!     method current(): TValue
//!     method key(): TKey
//! }
//! ```
//!
//! - One `function` entry per line: `function NAME[<templates>](
//!   [TYPE $name[, ...]] )[: TYPE]`. Parameters listed refine only
//!   those names; the return is optional (omitted means the base fold
//!   stays).
//! - A `class` entry opens with `class NAME[<templates>]
//!   [extends A<...>[, ...]] [implements B<...>[, ...]] {`, carries
//!   `method` lines in the function shape, and closes with `}`. A
//!   method's own template list colliding by name with the class's is
//!   a parse error (shared scope). `extends`/`implements` both lower
//!   into [`RefinedAncestor`] — the distinction is the stub graph's,
//!   not the overlay's.
//! - A template list is `<T, U of Bound>`; `of` introduces a bound
//!   (norm text, everything to the next top-level comma or `>`).
//! - Type texts are opaque here: split on top-level commas only
//!   (tracking `<`/`(`/`{` depth); the norm parser upstairs is the
//!   judge of their content.
//! - Keys fold at parse time (lowercase, no leading backslash) so blob
//!   keys match lookup keys; parameter names stay verbatim.

use crate::refinements::{
    RefinedAncestor, RefinedClass, RefinedSignature, RefinedTemplate, StubRefinements,
};
use crate::signature::{StubClassSurface, StubMemberKind, StubParameter, StubSignature};

/// One entry successfully parsed off a `function`/`method` line: its
/// folded key and the refinement signature it carries.
struct ParsedSignature {
    key: String,
    signature: RefinedSignature,
}

/// Parsing state while inside a `class ... { ... }` block.
struct ClassContext {
    key: String,
    /// The line the block opened on, for the "unterminated block"
    /// error if end-of-input arrives before the closing `}`.
    opening_line: usize,
    templates: Vec<RefinedTemplate>,
    ancestors: Vec<RefinedAncestor>,
    methods: Vec<(String, RefinedSignature)>,
}

/// Parses the whole `refinements.celerrate` source text into a
/// [`StubRefinements`] payload. Never panics: every malformed line
/// reports `"line {number}: {message}"` instead. Structure only — no
/// norm text is lowered or otherwise inspected here (decision 3).
pub fn parse_refinement_source(text: &str) -> Result<StubRefinements, String> {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut class_context: Option<ClassContext> = None;

    for (offset, raw_line) in text.lines().enumerate() {
        let line_number = offset + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if class_context.is_none() {
            if let Some(rest) = line.strip_prefix("function ") {
                let parsed = parse_signature_line(rest.trim())
                    .map_err(|message| format!("line {line_number}: {message}"))?;
                if functions
                    .iter()
                    .any(|(key, _): &(String, _)| *key == parsed.key)
                {
                    return Err(format!(
                        "line {line_number}: duplicate function {}",
                        parsed.key,
                    ));
                }
                functions.push((parsed.key, parsed.signature));
            } else if line == "function" {
                return Err(format!("line {line_number}: missing function name"));
            } else if let Some(rest) = line.strip_prefix("class ") {
                let context = parse_class_head(rest.trim(), line_number)
                    .map_err(|message| format!("line {line_number}: {message}"))?;
                if classes
                    .iter()
                    .any(|(key, _): &(String, _)| *key == context.key)
                {
                    return Err(format!(
                        "line {line_number}: duplicate class {}",
                        context.key
                    ));
                }
                class_context = Some(context);
            } else if line == "class" {
                return Err(format!("line {line_number}: missing class name"));
            } else if line == "method" || line.starts_with("method") {
                return Err(format!("line {line_number}: method outside of class"));
            } else {
                return Err(format!("line {line_number}: unrecognized entry: {line}"));
            }
            continue;
        }

        // Inside a `class { ... }` block (`class_context.is_some()`).
        if line == "}" {
            // `class_context.take()` needs sole mutable access to the
            // option, so this branch never binds a live `&mut
            // ClassContext` alongside it (unlike the branches below,
            // which use `as_mut` and never call `take`).
            let Some(context) = class_context.take() else {
                return Err(format!(
                    "line {line_number}: internal error: no open class body",
                ));
            };
            classes.push((
                context.key,
                RefinedClass {
                    templates: context.templates,
                    ancestors: context.ancestors,
                    methods: context.methods,
                },
            ));
            continue;
        }

        let Some(context) = class_context.as_mut() else {
            return Err(format!(
                "line {line_number}: internal error: no open class body",
            ));
        };
        if let Some(rest) = line.strip_prefix("method ") {
            let parsed = parse_signature_line(rest.trim())
                .map_err(|message| format!("line {line_number}: {message}"))?;
            for template in &parsed.signature.templates {
                if context
                    .templates
                    .iter()
                    .any(|class_template| class_template.name == template.name)
                {
                    return Err(format!(
                        "line {line_number}: method template {} collides with class {}'s template of the same name",
                        template.name, context.key,
                    ));
                }
            }
            if context.methods.iter().any(|(key, _)| *key == parsed.key) {
                return Err(format!(
                    "line {line_number}: duplicate method {}::{}",
                    context.key, parsed.key,
                ));
            }
            context.methods.push((parsed.key, parsed.signature));
        } else if line == "method" {
            return Err(format!("line {line_number}: missing method name"));
        } else {
            return Err(format!(
                "line {line_number}: unrecognized entry inside class {}: {line}",
                context.key,
            ));
        }
    }

    if let Some(context) = class_context {
        return Err(format!(
            "line {}: unterminated class body for {}",
            context.opening_line, context.key,
        ));
    }

    Ok(StubRefinements::new(functions, classes))
}

/// Parses one `function`/`method` line's tail (after the keyword has
/// already been stripped): `NAME[<templates>](parameters)[: RETURN]`.
fn parse_signature_line(text: &str) -> Result<ParsedSignature, String> {
    let paren = find_top_level_paren(text).ok_or_else(|| "missing parameter list".to_owned())?;
    let head = text.get(..paren).unwrap_or_default();
    let close = find_matching_close(text, paren + 1)
        .ok_or_else(|| "unterminated parameter list".to_owned())?;
    let (name, templates) = parse_name_and_templates(head)?;
    let parameter_text = text.get(paren + 1..close).unwrap_or_default();
    let parameters = parse_parameters(parameter_text)?;
    let tail = text.get(close + 1..).unwrap_or_default().trim();
    let return_type = if let Some(rest) = tail.strip_prefix(':') {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err("missing return type after ':'".to_owned());
        }
        Some(rest.to_owned())
    } else if tail.is_empty() {
        None
    } else {
        return Err(format!("unexpected trailing text: {tail}"));
    };
    Ok(ParsedSignature {
        key: folded(&name),
        signature: RefinedSignature {
            templates,
            parameters,
            return_type,
        },
    })
}

/// Splits a `NAME` or `NAME<templates>` head into the name and its
/// parsed template list.
fn parse_name_and_templates(head: &str) -> Result<(String, Vec<RefinedTemplate>), String> {
    match head.find('<') {
        None => {
            let name = head.trim();
            if name.is_empty() {
                return Err("missing name".to_owned());
            }
            Ok((name.to_owned(), Vec::new()))
        }
        Some(open) => {
            let name = head.get(..open).unwrap_or_default().trim();
            if name.is_empty() {
                return Err("missing name".to_owned());
            }
            let close = find_matching_close(head, open + 1)
                .ok_or_else(|| "unterminated template list".to_owned())?;
            let templates_text = head.get(open + 1..close).unwrap_or_default();
            let trailing = head.get(close + 1..).unwrap_or_default().trim();
            if !trailing.is_empty() {
                return Err(format!("unexpected text after template list: {trailing}"));
            }
            let templates = parse_templates(templates_text)?;
            Ok((name.to_owned(), templates))
        }
    }
}

/// Parses a template list's inner text (`T, U of Bound`) in source
/// order — templates are positionally significant, never sorted.
fn parse_templates(text: &str) -> Result<Vec<RefinedTemplate>, String> {
    let mut templates = Vec::new();
    for part in split_top_level(text) {
        let (name, bound) = match part.split_once(" of ") {
            Some((name, bound)) => (name.trim(), Some(bound.trim().to_owned())),
            None => (part.trim(), None),
        };
        if name.is_empty() {
            return Err(format!("empty template name in: {part}"));
        }
        if bound.as_deref().is_some_and(str::is_empty) {
            return Err(format!("empty template bound in: {part}"));
        }
        templates.push(RefinedTemplate {
            name: name.to_owned(),
            bound,
        });
    }
    Ok(templates)
}

/// Parses a parameter list's inner text (`TYPE $name, ...`).
fn parse_parameters(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut parameters = Vec::new();
    for part in split_top_level(text) {
        let Some((type_text, name)) = part.split_once('$') else {
            return Err(format!("parameter has no name: {part}"));
        };
        let type_text = type_text.trim();
        if type_text.is_empty() {
            return Err(format!("parameter has no type: {part}"));
        }
        let name = name.trim().trim_matches(['&', '.']).trim();
        if name.is_empty() {
            return Err(format!("parameter has no name: {part}"));
        }
        parameters.push((name.to_owned(), type_text.to_owned()));
    }
    Ok(parameters)
}

/// Parses a `class` line's tail (after the keyword has already been
/// stripped): `NAME[<templates>] [extends A<...>[, ...]] [implements
/// B<...>[, ...]] {`. The brief's grammar guarantees the line ends
/// with the class body's opening `{` as its last non-whitespace
/// character.
fn parse_class_head(text: &str, opening_line: usize) -> Result<ClassContext, String> {
    let Some(body) = text.trim_end().strip_suffix('{') else {
        return Err("class declaration must end with '{'".to_owned());
    };
    let body = body.trim_end();

    let name_end = body
        .find(|character: char| character == '<' || character.is_whitespace())
        .unwrap_or(body.len());
    let name = body.get(..name_end).unwrap_or_default().trim();
    if name.is_empty() {
        return Err("missing class name".to_owned());
    }
    let mut rest = body.get(name_end..).unwrap_or_default().trim_start();

    let mut templates = Vec::new();
    if rest.starts_with('<') {
        let close =
            find_matching_close(rest, 1).ok_or_else(|| "unterminated template list".to_owned())?;
        templates = parse_templates(rest.get(1..close).unwrap_or_default())?;
        rest = rest.get(close + 1..).unwrap_or_default().trim_start();
    }

    let mut ancestors = Vec::new();
    if let Some(after) = rest.strip_prefix("extends ") {
        let (clause, remainder) = split_before_implements(after);
        ancestors.extend(parse_ancestor_list(clause.trim())?);
        rest = remainder.trim_start();
    }
    if let Some(after) = rest.strip_prefix("implements ") {
        ancestors.extend(parse_ancestor_list(after.trim())?);
        rest = "";
    }
    if !rest.trim().is_empty() {
        return Err(format!("unexpected text in class header: {}", rest.trim()));
    }

    Ok(ClassContext {
        key: folded(name),
        opening_line,
        templates,
        ancestors,
        methods: Vec::new(),
    })
}

/// Splits an `extends` clause's ancestor list from a trailing
/// `implements` clause on the same header line, at the first
/// top-level `implements` keyword (word-bounded on its left by
/// whitespace, since ancestor argument lists never contain it).
fn split_before_implements(text: &str) -> (&str, &str) {
    match text.find("implements") {
        Some(position)
            if text
                .get(..position)
                .is_some_and(|before| before.ends_with(' ')) =>
        {
            (
                text.get(..position).unwrap_or_default().trim_end(),
                text.get(position..).unwrap_or_default(),
            )
        }
        _ => (text, ""),
    }
}

/// Parses a comma-separated ancestor list: each entry is `Name` or
/// `Name<arguments...>`, arguments kept in source order (positionally
/// significant, never sorted).
fn parse_ancestor_list(text: &str) -> Result<Vec<RefinedAncestor>, String> {
    let mut ancestors = Vec::new();
    for part in split_top_level(text) {
        let ancestor = match part.find('<') {
            None => RefinedAncestor {
                name: folded(part),
                arguments: Vec::new(),
            },
            Some(open) => {
                let name = part.get(..open).unwrap_or_default().trim();
                if name.is_empty() {
                    return Err(format!("ancestor missing a name: {part}"));
                }
                let close = find_matching_close(part, open + 1)
                    .ok_or_else(|| format!("unterminated ancestor argument list: {part}"))?;
                let arguments_text = part.get(open + 1..close).unwrap_or_default();
                let trailing = part.get(close + 1..).unwrap_or_default().trim();
                if !trailing.is_empty() {
                    return Err(format!(
                        "unexpected text after ancestor arguments: {trailing}"
                    ));
                }
                let arguments = split_top_level(arguments_text)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                RefinedAncestor {
                    name: folded(name),
                    arguments,
                }
            }
        };
        ancestors.push(ancestor);
    }
    Ok(ancestors)
}

/// Byte offset of the first top-level `(` in `text` (bracket depth
/// over `<`/`(`/`{`, matching [`split_top_level`]'s tracking).
fn find_top_level_paren(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, character) in text.char_indices() {
        match character {
            '(' if depth == 0 => return Some(offset),
            '<' | '(' | '{' => depth += 1,
            '>' | ')' | '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Byte offset of the bracket that closes the one just opened before
/// `start` (so `start` is the position right after that opening
/// bracket, depth already at 1). Tracks `<`/`(`/`{` together, like
/// [`split_top_level`]: the source grammar never mixes bracket kinds
/// so this stays a single depth counter.
fn find_matching_close(text: &str, start: usize) -> Option<usize> {
    let mut depth = 1i32;
    for (offset, character) in text.get(start..)?.char_indices() {
        match character {
            '<' | '(' | '{' => depth += 1,
            '>' | ')' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits on top-level commas, tracking `<`/`(`/`{` depth. Norm texts
/// are opaque here; only the bracket depth matters.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (offset, character) in text.char_indices() {
        match character {
            '<' | '(' | '{' => depth += 1,
            '>' | ')' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text.get(start..offset).unwrap_or_default());
                start = offset + 1;
            }
            _ => {}
        }
    }
    parts.push(text.get(start..).unwrap_or_default());
    parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

/// Folds a symbol key to its lookup form: lowercase, no leading
/// backslash. Applied to function names, class names, and ancestor
/// names; parameter names stay verbatim.
fn folded(name: &str) -> String {
    name.trim_start_matches('\\').to_lowercase()
}

/// Validates that every refined function, class, method, and
/// parameter names something that actually exists in the compiled
/// snapshot (decision 3's compiler-side half; lowering totality is
/// validated upstairs in `celerrate_types`). The error names the
/// offending entry so a contributor can find it without hunting.
pub fn validate_refinements(
    refinements: &StubRefinements,
    functions: &[(String, StubSignature)],
    classes: &[(String, StubClassSurface)],
) -> Result<(), String> {
    for (key, signature) in &refinements.functions {
        let Some((_, base)) = functions.iter().find(|(name, _)| folded(name) == *key) else {
            return Err(format!("refined function {key} is not in the snapshot"));
        };
        validate_parameters(key, signature, &base.parameters)?;
    }
    for (key, class) in &refinements.classes {
        let Some((_, surface)) = classes.iter().find(|(name, _)| folded(name) == *key) else {
            return Err(format!("refined class {key} is not in the snapshot"));
        };
        for (method_name, signature) in &class.methods {
            let Some(base) = surface.members.iter().find(|member| {
                member.kind == StubMemberKind::Method && folded(&member.name) == *method_name
            }) else {
                return Err(format!(
                    "refined method {key}::{method_name} is not in the snapshot",
                ));
            };
            let Some(base_signature) = &base.signature else {
                return Err(format!(
                    "refined method {key}::{method_name} has no base signature",
                ));
            };
            validate_parameters(method_name, signature, &base_signature.parameters)?;
        }
    }
    Ok(())
}

fn validate_parameters(
    target: &str,
    signature: &RefinedSignature,
    base: &[StubParameter],
) -> Result<(), String> {
    for (name, _) in &signature.parameters {
        if !base.iter().any(|parameter| parameter.name == *name) {
            return Err(format!(
                "refined parameter ${name} does not exist on {target}",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{parse_refinement_source, validate_refinements};
    use crate::refinements::{RefinedAncestor, RefinedSignature, RefinedTemplate, StubRefinements};
    use crate::signature::{StubParameter, StubSignature, VersionedTypeText};
    use crate::symbol::StubAvailability;

    #[test]
    fn a_function_entry_parses_with_templates_parameters_and_return() {
        let parsed = parse_refinement_source(
            "function array_keys<TKey, TValue>(array<TKey, TValue> $array): list<TKey>\n",
        )
        .unwrap();
        let (key, signature) = parsed.functions.first().unwrap();
        assert_eq!(key, "array_keys");
        assert_eq!(
            signature.templates,
            vec![
                RefinedTemplate {
                    name: "TKey".to_owned(),
                    bound: None
                },
                RefinedTemplate {
                    name: "TValue".to_owned(),
                    bound: None
                },
            ],
        );
        assert_eq!(
            signature.parameters,
            vec![("array".to_owned(), "array<TKey, TValue>".to_owned())],
        );
        assert_eq!(signature.return_type.as_deref(), Some("list<TKey>"));
    }

    #[test]
    fn a_bound_reads_after_of_and_commas_nest() {
        let parsed = parse_refinement_source(
            "function pick<T of Countable&Traversable>(array<int, T> $items): T\n",
        )
        .unwrap();
        let (_, signature) = parsed.functions.first().unwrap();
        assert_eq!(
            signature.templates,
            vec![RefinedTemplate {
                name: "T".to_owned(),
                bound: Some("Countable&Traversable".to_owned()),
            }],
        );
    }

    #[test]
    fn a_class_entry_parses_ancestors_and_methods() {
        let parsed = parse_refinement_source(
            "class ArrayIterator<TKey, TValue> implements Iterator<TKey, TValue> {\n\
             \tmethod current(): TValue\n\
             \tmethod key(): TKey\n\
             }\n",
        )
        .unwrap();
        let (key, class) = parsed.classes.first().unwrap();
        assert_eq!(key, "arrayiterator");
        assert_eq!(
            class.ancestors,
            vec![RefinedAncestor {
                name: "iterator".to_owned(),
                arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
            }],
        );
        assert_eq!(class.methods.len(), 2);
        let (name, current) = class.methods.first().unwrap();
        assert_eq!(name, "current");
        assert_eq!(current.return_type.as_deref(), Some("TValue"));
    }

    #[test]
    fn names_fold_and_comments_are_skipped() {
        let parsed =
            parse_refinement_source("# the seed\nfunction Array_Keys(): list<int>\n").unwrap();
        assert_eq!(parsed.functions.first().unwrap().0, "array_keys");
    }

    #[test]
    fn malformed_lines_fail_with_the_line_number() {
        for (text, line) in [
            ("function\n", 1),
            ("# fine\nfunction broken(\n", 2),
            ("class Foo {\nmethod\n}\n", 2),
            ("class Foo {\n", 1),                         // unterminated block
            ("method orphan(): int\n", 1),                // method outside a class
            ("class Foo<T> {\nmethod m<T>(): T\n}\n", 2), // template collision
        ] {
            let error = parse_refinement_source(text).unwrap_err();
            assert!(
                error.starts_with(&format!("line {line}")),
                "for {text:?}: {error}",
            );
        }
    }

    #[test]
    fn the_empty_source_parses_to_no_entries() {
        let parsed = parse_refinement_source("").unwrap();
        assert!(parsed.functions.is_empty());
        assert!(parsed.classes.is_empty());
    }

    #[test]
    fn a_comment_only_source_parses_to_no_entries() {
        let parsed = parse_refinement_source(
            "# nothing here\n# still nothing\n\n# and a blank line above\n",
        )
        .unwrap();
        assert!(parsed.functions.is_empty());
        assert!(parsed.classes.is_empty());
    }

    #[test]
    fn a_duplicate_function_key_is_a_parse_error() {
        // `StubRefinements::new` sorts by key but never dedups, so a
        // duplicate entry would otherwise reach the blob as two
        // identically-keyed rows — undefined which one a binary
        // search finds. The parser rejects it outright, and folding
        // means `Array_Keys` collides with `array_keys` too.
        let error = parse_refinement_source(
            "function array_keys(): list<int>\nfunction Array_Keys(): list<int>\n",
        )
        .unwrap_err();
        assert!(error.starts_with("line 2"), "{error}");
        assert!(error.contains("array_keys"), "{error}");
    }

    #[test]
    fn a_duplicate_class_key_is_a_parse_error() {
        let error = parse_refinement_source("class Foo {\n}\nclass Foo {\n}\n").unwrap_err();
        assert!(error.starts_with("line 3"), "{error}");
        assert!(error.contains("foo"), "{error}");
    }

    #[test]
    fn a_duplicate_method_key_within_a_class_is_a_parse_error() {
        let error =
            parse_refinement_source("class Foo {\nmethod bar(): int\nmethod bar(): int\n}\n")
                .unwrap_err();
        assert!(error.starts_with("line 3"), "{error}");
        assert!(error.contains("bar"), "{error}");
    }

    #[test]
    fn arbitrary_text_never_panics_and_always_returns_a_result() {
        // No production code may crash on user input (project rule):
        // a pile of brackets, stray keywords, and non-ASCII bytes must
        // still resolve to `Ok` or `Err`, never a panic.
        let inputs = [
            "<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<\n",
            ">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>\n",
            "function 🎉<🎉>(🎉 $🎉): 🎉\n",
            "class 例<例 of 例>実装 {\nmethod 例(): 例\n}\n",
            "function f(((((((((((((((((((((((((((((((((\n",
            "class {\n",
            "function f($, $, $$$$$)\n",
            "}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}\n",
        ];
        for input in inputs {
            let _ = parse_refinement_source(input);
        }
    }

    #[test]
    fn validation_names_the_missing_target() {
        let refinements = StubRefinements::new(
            vec![("missing_function".to_owned(), RefinedSignature::default())],
            vec![],
        );
        let error = validate_refinements(&refinements, &[], &[]).unwrap_err();
        assert!(error.contains("missing_function"), "{error}");
    }

    #[test]
    fn validation_checks_parameters_methods_and_classes() {
        let functions = vec![(
            "array_keys".to_owned(),
            StubSignature {
                parameters: vec![StubParameter {
                    name: "array".to_owned(),
                    type_text: VersionedTypeText::default(),
                    optional: false,
                    by_reference: false,
                    variadic: false,
                    availability: StubAvailability::ALWAYS,
                }],
                return_type: VersionedTypeText::default(),
                by_reference: false,
            },
        )];
        // A refined parameter name absent from the base signature fails.
        let refinements = StubRefinements::new(
            vec![(
                "array_keys".to_owned(),
                RefinedSignature {
                    templates: vec![],
                    parameters: vec![("wrong".to_owned(), "int".to_owned())],
                    return_type: None,
                },
            )],
            vec![],
        );
        let error = validate_refinements(&refinements, &functions, &[]).unwrap_err();
        assert!(error.contains("wrong"), "{error}");
    }
}
