//! The Celerrate refinements overlay: enriched stub signatures
//! written in the internal norm, compiled into the blob's third
//! section at build time and consulted upstairs by `celerrate_types`
//! at the stub-signature fold. Texts are opaque strings here — this
//! crate sits below the lattice and never lowers them (the compiler
//! validates existence, the types crate validates lowering
//! totality).

use crate::blob::{Reader, StubBlobError};

/// One declared template: `TKey`, or `T of Foo` (the bound is a norm
/// text, lowered upstairs).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RefinedTemplate {
    pub name: String,
    pub bound: Option<String>,
}

/// A per-element signature override: only the named parameters and
/// (when present) the return are replaced; everything else keeps the
/// base stub's delta fold. Version-agnostic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RefinedSignature {
    pub templates: Vec<RefinedTemplate>,
    /// Parameter name (without `$`) to norm text.
    pub parameters: Vec<(String, String)>,
    pub return_type: Option<String>,
}

/// One generic ancestor fixed by a class refinement:
/// `implements Iterator<TKey, TValue>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RefinedAncestor {
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RefinedClass {
    pub templates: Vec<RefinedTemplate>,
    pub ancestors: Vec<RefinedAncestor>,
    /// Method name (folded) to signature refinement, sorted by name
    /// inside `StubRefinements::new` (methods carry no positional
    /// meaning, unlike `templates` and `ancestors[].arguments`).
    pub methods: Vec<(String, RefinedSignature)>,
}

/// The whole overlay, keyed by folded symbol keys: `functions` and
/// `classes` are sorted by key, and each class's `methods` is sorted
/// by name too, so lookups binary-search and the blob encoding is
/// deterministic. Duplicate keys collapse to the first entry after the
/// sort, matching `StubIndex::new`; the sole production producer already
/// rejects duplicates, so this is defense in depth for programmatic callers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubRefinements {
    pub functions: Vec<(String, RefinedSignature)>,
    pub classes: Vec<(String, RefinedClass)>,
}

impl StubRefinements {
    pub fn new(
        mut functions: Vec<(String, RefinedSignature)>,
        mut classes: Vec<(String, RefinedClass)>,
    ) -> Self {
        functions.sort_by(|left, right| left.0.cmp(&right.0));
        functions.dedup_by(|second, first| first.0 == second.0);
        classes.sort_by(|left, right| left.0.cmp(&right.0));
        classes.dedup_by(|second, first| first.0 == second.0);
        for (_, class) in &mut classes {
            class.methods.sort_by(|left, right| left.0.cmp(&right.0));
            class.methods.dedup_by(|second, first| first.0 == second.0);
        }
        Self { functions, classes }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.classes.is_empty()
    }
}

pub(crate) fn encode_refinements(refinements: &StubRefinements, bytes: &mut Vec<u8>) {
    write_u32(bytes, refinements.functions.len());
    for (key, signature) in &refinements.functions {
        write_text(bytes, key);
        encode_signature(signature, bytes);
    }
    write_u32(bytes, refinements.classes.len());
    for (key, class) in &refinements.classes {
        write_text(bytes, key);
        encode_templates(&class.templates, bytes);
        write_u32(bytes, class.ancestors.len());
        for ancestor in &class.ancestors {
            write_text(bytes, &ancestor.name);
            write_u32(bytes, ancestor.arguments.len());
            for argument in &ancestor.arguments {
                write_text(bytes, argument);
            }
        }
        write_u32(bytes, class.methods.len());
        for (name, signature) in &class.methods {
            write_text(bytes, name);
            encode_signature(signature, bytes);
        }
    }
}

fn encode_signature(signature: &RefinedSignature, bytes: &mut Vec<u8>) {
    encode_templates(&signature.templates, bytes);
    write_u32(bytes, signature.parameters.len());
    for (name, text) in &signature.parameters {
        write_text(bytes, name);
        write_text(bytes, text);
    }
    write_optional_text(bytes, signature.return_type.as_deref());
}

fn encode_templates(templates: &[RefinedTemplate], bytes: &mut Vec<u8>) {
    write_u32(bytes, templates.len());
    for template in templates {
        write_text(bytes, &template.name);
        write_optional_text(bytes, template.bound.as_deref());
    }
}

fn write_u32(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u32).to_le_bytes());
}

fn write_text(bytes: &mut Vec<u8>, text: &str) {
    write_u32(bytes, text.len());
    bytes.extend_from_slice(text.as_bytes());
}

fn write_optional_text(bytes: &mut Vec<u8>, text: Option<&str>) {
    match text {
        Some(text) => {
            bytes.push(1);
            write_text(bytes, text);
        }
        None => bytes.push(0),
    }
}

pub(crate) fn decode_refinements(bytes: &[u8]) -> Result<StubRefinements, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let function_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut functions = Vec::new();
    for _ in 0..function_count {
        let key = reader.string().ok_or(StubBlobError::MalformedSection)?;
        functions.push((key, decode_signature(&mut reader)?));
    }
    let class_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut classes = Vec::new();
    for _ in 0..class_count {
        let key = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let templates = decode_templates(&mut reader)?;
        let ancestor_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut ancestors = Vec::new();
        for _ in 0..ancestor_count {
            let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
            let argument_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
            let mut arguments = Vec::new();
            for _ in 0..argument_count {
                arguments.push(reader.string().ok_or(StubBlobError::MalformedSection)?);
            }
            ancestors.push(RefinedAncestor { name, arguments });
        }
        let method_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut methods = Vec::new();
        for _ in 0..method_count {
            let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
            methods.push((name, decode_signature(&mut reader)?));
        }
        classes.push((
            key,
            RefinedClass {
                templates,
                ancestors,
                methods,
            },
        ));
    }
    Ok(StubRefinements::new(functions, classes))
}

fn decode_signature(reader: &mut Reader<'_>) -> Result<RefinedSignature, StubBlobError> {
    let templates = decode_templates(reader)?;
    let parameter_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut parameters = Vec::new();
    for _ in 0..parameter_count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let text = reader.string().ok_or(StubBlobError::MalformedSection)?;
        parameters.push((name, text));
    }
    let return_type = decode_optional_text(reader)?;
    Ok(RefinedSignature {
        templates,
        parameters,
        return_type,
    })
}

fn decode_templates(reader: &mut Reader<'_>) -> Result<Vec<RefinedTemplate>, StubBlobError> {
    let count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut templates = Vec::new();
    for _ in 0..count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let bound = decode_optional_text(reader)?;
        templates.push(RefinedTemplate { name, bound });
    }
    Ok(templates)
}

fn decode_optional_text(reader: &mut Reader<'_>) -> Result<Option<String>, StubBlobError> {
    match reader.u8().ok_or(StubBlobError::MalformedSection)? {
        0 => Ok(None),
        1 => Ok(Some(
            reader.string().ok_or(StubBlobError::MalformedSection)?,
        )),
        _ => Err(StubBlobError::MalformedSection),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn sample() -> StubRefinements {
        StubRefinements::new(
            vec![(
                "array_keys".to_owned(),
                RefinedSignature {
                    templates: vec![
                        RefinedTemplate {
                            name: "TKey".to_owned(),
                            bound: None,
                        },
                        RefinedTemplate {
                            name: "TValue".to_owned(),
                            bound: Some("object".to_owned()),
                        },
                    ],
                    parameters: vec![("array".to_owned(), "array<TKey, TValue>".to_owned())],
                    return_type: Some("list<TKey>".to_owned()),
                },
            )],
            vec![(
                "arrayiterator".to_owned(),
                RefinedClass {
                    templates: vec![
                        RefinedTemplate {
                            name: "TKey".to_owned(),
                            bound: None,
                        },
                        RefinedTemplate {
                            name: "TValue".to_owned(),
                            bound: None,
                        },
                    ],
                    ancestors: vec![RefinedAncestor {
                        name: "iterator".to_owned(),
                        arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
                    }],
                    methods: vec![(
                        "current".to_owned(),
                        RefinedSignature {
                            templates: vec![],
                            parameters: vec![],
                            return_type: Some("TValue".to_owned()),
                        },
                    )],
                },
            )],
        )
    }

    #[test]
    fn construction_sorts_by_key() {
        let refinements = StubRefinements::new(
            vec![
                ("b".to_owned(), RefinedSignature::default()),
                ("a".to_owned(), RefinedSignature::default()),
            ],
            vec![],
        );
        let keys: Vec<&str> = refinements
            .functions
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, ["a", "b"]);
    }

    #[test]
    fn construction_sorts_methods_by_name_within_each_class() {
        let refinements = StubRefinements::new(
            vec![],
            vec![(
                "arrayiterator".to_owned(),
                RefinedClass {
                    templates: vec![],
                    ancestors: vec![],
                    methods: vec![
                        ("valid".to_owned(), RefinedSignature::default()),
                        ("current".to_owned(), RefinedSignature::default()),
                        ("key".to_owned(), RefinedSignature::default()),
                    ],
                },
            )],
        );
        let class = &refinements
            .classes
            .iter()
            .find(|(key, _)| key == "arrayiterator")
            .unwrap()
            .1;
        let method_names: Vec<&str> = class
            .methods
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(method_names, ["current", "key", "valid"]);
    }

    #[test]
    fn duplicate_function_keys_collapse_to_the_first_entry() {
        let first = RefinedSignature {
            return_type: Some("int".to_owned()),
            ..RefinedSignature::default()
        };
        let second = RefinedSignature {
            return_type: Some("string".to_owned()),
            ..RefinedSignature::default()
        };
        let refinements = StubRefinements::new(
            vec![
                ("strlen".to_owned(), first.clone()),
                ("strlen".to_owned(), second),
            ],
            Vec::new(),
        );
        assert_eq!(refinements.functions, vec![("strlen".to_owned(), first)]);
    }

    #[test]
    fn duplicate_class_keys_collapse_to_the_first_entry() {
        let first = RefinedClass {
            templates: vec![RefinedTemplate {
                name: "T".to_owned(),
                bound: None,
            }],
            ..RefinedClass::default()
        };
        let refinements = StubRefinements::new(
            Vec::new(),
            vec![
                ("iterator".to_owned(), first.clone()),
                ("iterator".to_owned(), RefinedClass::default()),
            ],
        );
        assert_eq!(refinements.classes, vec![("iterator".to_owned(), first)]);
    }

    #[test]
    fn duplicate_method_names_collapse_to_the_first_entry_within_a_class() {
        let first = RefinedSignature {
            return_type: Some("static".to_owned()),
            ..RefinedSignature::default()
        };
        let class = RefinedClass {
            methods: vec![
                ("current".to_owned(), first.clone()),
                ("current".to_owned(), RefinedSignature::default()),
            ],
            ..RefinedClass::default()
        };
        let refinements = StubRefinements::new(Vec::new(), vec![("iterator".to_owned(), class)]);
        assert_eq!(
            refinements.classes,
            vec![(
                "iterator".to_owned(),
                RefinedClass {
                    methods: vec![("current".to_owned(), first)],
                    ..RefinedClass::default()
                }
            )],
        );
    }

    #[test]
    fn the_payload_round_trips_through_its_encoding() {
        let refinements = sample();
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        let decoded = super::decode_refinements(&bytes).unwrap();
        assert_eq!(decoded, refinements);
    }

    #[test]
    fn a_truncated_payload_reports_malformed_never_panics() {
        let refinements = sample();
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        for length in 0..bytes.len() {
            let truncated = bytes.get(..length).unwrap_or_default();
            assert!(super::decode_refinements(truncated).is_err());
        }
    }

    #[test]
    fn the_empty_payload_round_trips() {
        let refinements = StubRefinements::empty();
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        let decoded = super::decode_refinements(&bytes).unwrap();
        assert_eq!(decoded, refinements);
        assert!(decoded.is_empty());
    }

    #[test]
    fn an_empty_byte_slice_is_malformed_not_a_missing_section() {
        // The tolerance rule ("no overlays section at all decodes as
        // empty refinements") lives in `blob::decode`, not here: an
        // empty *section payload* is still missing its mandatory
        // counts, so it is a decode error.
        assert!(super::decode_refinements(&[]).is_err());
    }

    #[test]
    fn a_single_function_entry_with_no_class_round_trips() {
        let refinements = StubRefinements::new(
            vec![(
                "strlen".to_owned(),
                RefinedSignature {
                    templates: vec![],
                    parameters: vec![],
                    return_type: Some("int".to_owned()),
                },
            )],
            vec![],
        );
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        let decoded = super::decode_refinements(&bytes).unwrap();
        assert_eq!(decoded, refinements);
        assert_eq!(decoded.functions.len(), 1);
        assert!(decoded.classes.is_empty());
    }

    #[test]
    fn an_invalid_optional_text_discriminant_is_malformed() {
        // `write_optional_text` only ever emits 0 or 1; a decoder that
        // silently treated anything else as `None` would swallow
        // corruption instead of reporting it.
        let mut bytes = Vec::new();
        write_u32(&mut bytes, 1); // one function
        write_text(&mut bytes, "f");
        write_u32(&mut bytes, 0); // no templates
        write_u32(&mut bytes, 0); // no parameters
        bytes.push(2); // invalid optional-text discriminant for return_type
        assert!(super::decode_refinements(&bytes).is_err());
    }
}
