//! The inherited PHPDoc type-expression grammar is complete: union,
//! intersection, nullable-inside-suffix, `[]` and offset suffixes,
//! generics with dropped call-site variance, shapes with unsealed tails,
//! callables with required returns and dropped parameter names, const fetches,
//! `$this`, and conditional types.
//!
//! The parser answers `None` for: out-of-grammar text, unterminated constructs
//! (incomplete `<...>`, `{...}`, `(...)`, or conditional expressions), and
//! nesting past the depth guard (per construct, never per annotation).
//!
//! The parser is a recursive descent over the token stream (Task 1) with
//! a depth guard: adversarial nesting must not overflow the stack.
//! Entry points `parse_type_expression_text` consumes a whole input;
//! `parse_type_expression_prefix` reports the consumed byte length so
//! the tag layer can split type from prose.

mod parser;
mod tokens;

/// A parsed type expression of the inherited PHPDoc dialect family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpression {
    Name(String),
    Nullable(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    ArrayOf(Box<TypeExpression>),
    IntLiteral(i64),
    /// The written text; lowering parses it (`Eq` stays derivable).
    FloatLiteral(String),
    StringLiteral(String),
    Generic {
        base: String,
        arguments: Vec<TypeExpression>,
    },
    Shape {
        base: String,
        fields: Vec<ShapeFieldExpression>,
        unsealed: Option<UnsealedTail>,
    },
    Callable {
        base: String,
        /// Callable-scoped template names — decision 12: their
        /// occurrences inside the signature lower to `mixed`.
        templates: Vec<String>,
        parameters: Vec<CallableParameterExpression>,
        return_type: Box<TypeExpression>,
    },
    ConstFetch {
        class: String,
        constant: String,
    },
    This,
    Offset {
        base: Box<TypeExpression>,
        offset: Box<TypeExpression>,
    },
    Conditional {
        subject: ConditionalSubject,
        negated: bool,
        target: Box<TypeExpression>,
        then_branch: Box<TypeExpression>,
        otherwise_branch: Box<TypeExpression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeFieldExpression {
    pub key: Option<ShapeKeyExpression>,
    pub optional: bool,
    pub value: TypeExpression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeKeyExpression {
    Integer(i64),
    String(String),
    Identifier(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsealedTail {
    pub key: Option<Box<TypeExpression>>,
    pub value: Option<Box<TypeExpression>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableParameterExpression {
    pub parameter_type: TypeExpression,
    pub by_reference: bool,
    pub variadic: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalSubject {
    /// A bare name — a template variable if one is in scope at
    /// lowering time, otherwise undecided.
    Template(String),
    /// `$param` — undecided until plan 6 evaluates call sites.
    Parameter(String),
}

/// Parses `text` as one type expression consuming the whole input
/// (trailing whitespace allowed); anything left over, anything outside
/// the grammar, or anything nested past the depth guard answers
/// `None`.
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression> {
    let (expression, consumed) = parse_type_expression_prefix(text)?;
    let remainder = text.get(consumed..)?;
    if remainder.trim().is_empty() {
        Some(expression)
    } else {
        None
    }
}

/// Parses a maximal well-formed type expression from the start of
/// `text` and reports the consumed byte length — the tag layer takes
/// the type from the prefix and the variable or prose from the
/// remainder. Grammar failure anywhere answers `None` for the whole
/// expression: loss is per construct, never partially recovered.
pub fn parse_type_expression_prefix(text: &str) -> Option<(TypeExpression, usize)> {
    let tokens = tokens::tokenize(text);
    let mut cursor = parser::Parser::new(&tokens);
    let expression = parser::parse_type(&mut cursor, 0)?;
    let consumed = cursor.consumed_end()?;
    Some((expression, consumed))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn nullable_binds_inside_the_array_suffix() {
        // The reference parses `?int[]` as an array of nullable int
        // ((?int)[]), not a nullable array — decision 4.
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("?int[]"),
            Some(ArrayOf(Box::new(Nullable(Box::new(Name(
                "int".to_owned()
            )))))),
        );
    }

    #[test]
    fn prefix_parsing_reports_the_consumed_length() {
        let (expression, consumed) =
            parse_type_expression_prefix("int|string $x the identifier").unwrap();
        assert_eq!(
            expression,
            TypeExpression::Union(vec![
                TypeExpression::Name("int".to_owned()),
                TypeExpression::Name("string".to_owned()),
            ]),
        );
        assert_eq!(consumed, "int|string".len());
        assert!(parse_type_expression_prefix("$x only prose").is_none());
    }

    #[test]
    fn the_standard_grammar_parses() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("int"),
            Some(Name("int".to_owned()))
        );
        assert_eq!(
            parse_type_expression_text("?string"),
            Some(Nullable(Box::new(Name("string".to_owned())))),
        );
        assert_eq!(
            parse_type_expression_text("int|null"),
            Some(Union(vec![Name("int".to_owned()), Name("null".to_owned())])),
        );
        assert_eq!(
            parse_type_expression_text("Countable&Traversable"),
            Some(Intersection(vec![
                Name("Countable".to_owned()),
                Name("Traversable".to_owned()),
            ])),
        );
        assert_eq!(
            parse_type_expression_text("User[]"),
            Some(ArrayOf(Box::new(Name("User".to_owned())))),
        );
        assert_eq!(
            parse_type_expression_text("(int|string)[]"),
            Some(ArrayOf(Box::new(Union(vec![
                Name("int".to_owned()),
                Name("string".to_owned()),
            ])))),
        );
        assert_eq!(
            parse_type_expression_text("\\App\\User"),
            Some(Name("\\App\\User".to_owned())),
        );
    }

    #[test]
    fn literals_parse() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("'active'"),
            Some(StringLiteral("active".to_owned())),
        );
        assert_eq!(parse_type_expression_text("42"), Some(IntLiteral(42)));
        assert_eq!(parse_type_expression_text("-1"), Some(IntLiteral(-1)));
        assert_eq!(
            parse_type_expression_text("1.5"),
            Some(FloatLiteral("1.5".to_owned())),
        );
        assert_eq!(
            parse_type_expression_text("'yes'|'no'"),
            Some(Union(vec![
                StringLiteral("yes".to_owned()),
                StringLiteral("no".to_owned()),
            ])),
        );
    }

    #[test]
    fn generics_parse_with_nesting_ranges_and_variance() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("array<int, string>"),
            Some(Generic {
                base: "array".to_owned(),
                arguments: vec![Name("int".to_owned()), Name("string".to_owned())],
            }),
        );
        assert_eq!(
            parse_type_expression_text("array<int, array<string, User>>"),
            Some(Generic {
                base: "array".to_owned(),
                arguments: vec![
                    Name("int".to_owned()),
                    Generic {
                        base: "array".to_owned(),
                        arguments: vec![Name("string".to_owned()), Name("User".to_owned())],
                    },
                ],
            }),
        );
        assert_eq!(
            parse_type_expression_text("class-string<T>"),
            Some(Generic {
                base: "class-string".to_owned(),
                arguments: vec![Name("T".to_owned())],
            }),
        );
        assert_eq!(
            parse_type_expression_text("int<1, max>"),
            Some(Generic {
                base: "int".to_owned(),
                arguments: vec![IntLiteral(1), Name("max".to_owned())],
            }),
        );
        // Call-site variance keywords are consumed and dropped.
        assert_eq!(
            parse_type_expression_text("Collection<covariant User>"),
            Some(Generic {
                base: "Collection".to_owned(),
                arguments: vec![Name("User".to_owned())],
            }),
        );
        // A bare `*` argument is the bivariant wildcard (the pinned
        // reference's "unknown, don't care" marker): it carries no
        // type of its own, so it lowers to `mixed`, same posture as
        // the dropped variance keywords above.
        assert_eq!(
            parse_type_expression_text("Foo<Bar, *>"),
            Some(Generic {
                base: "Foo".to_owned(),
                arguments: vec![Name("Bar".to_owned()), Name("mixed".to_owned())],
            }),
        );
    }

    #[test]
    fn shapes_parse_with_every_key_form() {
        let Some(TypeExpression::Shape {
            base,
            fields,
            unsealed,
        }) = parse_type_expression_text("array{id: int, name?: string, 'q': bool, 0: float}")
        else {
            panic!("expected a shape");
        };
        assert_eq!(base, "array");
        assert!(unsealed.is_none());
        assert_eq!(fields.len(), 4);
        assert_eq!(
            fields[0].key,
            Some(ShapeKeyExpression::Identifier("id".to_owned()))
        );
        assert!(!fields[0].optional);
        assert_eq!(
            fields[1].key,
            Some(ShapeKeyExpression::Identifier("name".to_owned()))
        );
        assert!(fields[1].optional);
        assert_eq!(
            fields[2].key,
            Some(ShapeKeyExpression::String("q".to_owned()))
        );
        assert_eq!(fields[3].key, Some(ShapeKeyExpression::Integer(0)));
    }

    #[test]
    fn tuples_empty_shapes_and_other_bases_parse() {
        let Some(TypeExpression::Shape { fields, .. }) =
            parse_type_expression_text("array{int, string}")
        else {
            panic!("expected a tuple shape");
        };
        assert!(fields.iter().all(|field| field.key.is_none()));
        assert!(matches!(
            parse_type_expression_text("array{}"),
            Some(TypeExpression::Shape { fields, unsealed: None, .. }) if fields.is_empty()
        ));
        for text in [
            "list{int, string}",
            "object{a: int}",
            "non-empty-array{a: int}",
            "non-empty-list{int}",
        ] {
            assert!(
                matches!(
                    parse_type_expression_text(text),
                    Some(TypeExpression::Shape { .. })
                ),
                "{text}",
            );
        }
        // Shape bases are case-insensitive.
        assert!(matches!(
            parse_type_expression_text("Array{a: int}"),
            Some(TypeExpression::Shape { .. })
        ));
        // A brace after a non-shape base is not a shape.
        assert_eq!(parse_type_expression_text("Foo{a: int}"), None);
        assert!(matches!(
            parse_type_expression_prefix("Foo{a: int}"),
            Some((TypeExpression::Name(name), 3)) if name == "Foo"
        ));
    }

    #[test]
    fn unsealed_tails_parse() {
        let Some(TypeExpression::Shape {
            unsealed: Some(tail),
            ..
        }) = parse_type_expression_text("array{a: int, ...}")
        else {
            panic!("expected an unsealed shape");
        };
        assert_eq!(
            tail,
            UnsealedTail {
                key: None,
                value: None
            }
        );
        let Some(TypeExpression::Shape {
            unsealed: Some(tail),
            ..
        }) = parse_type_expression_text("array{a: int, ...<string, bool>}")
        else {
            panic!("expected a typed unsealed tail");
        };
        assert_eq!(
            tail,
            UnsealedTail {
                key: Some(Box::new(TypeExpression::Name("string".to_owned()))),
                value: Some(Box::new(TypeExpression::Name("bool".to_owned()))),
            },
        );
    }

    #[test]
    fn callables_parse_with_parameter_flags_and_templates() {
        let Some(TypeExpression::Callable {
            base,
            templates,
            parameters,
            return_type,
        }) = parse_type_expression_text("callable(int, string&$out, User...$rest, bool=): ?string")
        else {
            panic!("expected a callable");
        };
        assert_eq!(base, "callable");
        assert!(templates.is_empty());
        assert_eq!(parameters.len(), 4);
        assert!(parameters[1].by_reference);
        assert!(parameters[2].variadic);
        assert!(parameters[3].optional);
        assert_eq!(
            *return_type,
            TypeExpression::Nullable(Box::new(TypeExpression::Name("string".to_owned()))),
        );

        let Some(TypeExpression::Callable {
            base, templates, ..
        }) = parse_type_expression_text("\\Closure<T of Foo>(T): T")
        else {
            panic!("expected a closure");
        };
        assert_eq!(base, "\\Closure");
        assert_eq!(templates, vec!["T".to_owned()]);
        // A bare `callable` without a signature stays a plain name.
        assert_eq!(
            parse_type_expression_text("callable"),
            Some(TypeExpression::Name("callable".to_owned())),
        );
        // A generic Closure without a signature stays a generic.
        assert!(matches!(
            parse_type_expression_text("Closure<int>"),
            Some(TypeExpression::Generic { .. })
        ));

        // Full types are parameter types: unions and intersections
        // survive next to the by-reference marker.
        let Some(TypeExpression::Callable { parameters, .. }) =
            parse_type_expression_text("callable(int|string): void")
        else {
            panic!("expected a callable with a union parameter");
        };
        assert_eq!(
            parameters[0].parameter_type,
            TypeExpression::Union(vec![
                TypeExpression::Name("int".to_owned()),
                TypeExpression::Name("string".to_owned()),
            ]),
        );
        let Some(TypeExpression::Callable { parameters, .. }) =
            parse_type_expression_text("callable(Countable&Traversable): void")
        else {
            panic!("expected a callable with an intersection parameter");
        };
        assert_eq!(
            parameters[0].parameter_type,
            TypeExpression::Intersection(vec![
                TypeExpression::Name("Countable".to_owned()),
                TypeExpression::Name("Traversable".to_owned()),
            ]),
        );

        // A trailing comma before the closing parenthesis is
        // tolerated, same as the shape and generic argument lists.
        let Some(TypeExpression::Callable { parameters, .. }) =
            parse_type_expression_text("callable(Bar,): void")
        else {
            panic!("expected a callable with a trailing-comma parameter list");
        };
        assert_eq!(
            parameters[0].parameter_type,
            TypeExpression::Name("Bar".to_owned()),
        );
    }

    #[test]
    fn const_fetches_this_and_offsets_parse() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("Foo::BAR"),
            Some(ConstFetch {
                class: "Foo".to_owned(),
                constant: "BAR".to_owned()
            }),
        );
        assert_eq!(
            parse_type_expression_text("Foo::*"),
            Some(ConstFetch {
                class: "Foo".to_owned(),
                constant: "*".to_owned()
            }),
        );
        assert_eq!(
            parse_type_expression_text("Foo::BAR_*"),
            Some(ConstFetch {
                class: "Foo".to_owned(),
                constant: "BAR_*".to_owned()
            }),
        );
        assert_eq!(parse_type_expression_text("$this"), Some(This));
        assert_eq!(
            parse_type_expression_text("T[K]"),
            Some(Offset {
                base: Box::new(Name("T".to_owned())),
                offset: Box::new(Name("K".to_owned())),
            }),
        );
        // A lone `$param` is not a type.
        assert_eq!(parse_type_expression_text("$param"), None);
    }

    #[test]
    fn offset_suffixes_roll_back_at_prose_boundaries() {
        // A whitespace-separated bracket is prose, not offset access
        // (the reference requires adjacency and rolls back on failure).
        assert_eq!(
            parse_type_expression_prefix("bool [true when the lock was acquired]"),
            Some((TypeExpression::Name("bool".to_owned()), 4)),
        );
        // A failed offset body rolls back to the base type.
        assert_eq!(
            parse_type_expression_prefix("int[|]"),
            Some((TypeExpression::Name("int".to_owned()), 3)),
        );
        // Adjacent, well-formed offset access still parses.
        assert!(matches!(
            parse_type_expression_text("T[K]"),
            Some(TypeExpression::Offset { .. })
        ));
    }

    #[test]
    fn conditional_types_parse_for_both_subjects() {
        let Some(TypeExpression::Conditional {
            subject, negated, ..
        }) = parse_type_expression_text("T is string ? int : bool")
        else {
            panic!("expected a conditional");
        };
        assert_eq!(subject, ConditionalSubject::Template("T".to_owned()));
        assert!(!negated);

        let Some(TypeExpression::Conditional {
            subject,
            negated,
            then_branch,
            ..
        }) = parse_type_expression_text("($flags is not 1 ? array<string> : string)")
        else {
            panic!("expected a parameter conditional");
        };
        assert_eq!(subject, ConditionalSubject::Parameter("flags".to_owned()));
        assert!(negated);
        assert!(matches!(*then_branch, TypeExpression::Generic { .. }));
    }

    #[test]
    fn dialect_constructs_and_garbage_answer_none() {
        for text in [
            "array{a: int",
            "array<int",
            "Foo<>",
            "",
            "|",
            "?",
            "int|",
            "((int)",
            "int string",
            "callable(int",
        ] {
            assert_eq!(parse_type_expression_text(text), None, "{text}");
        }
    }

    #[test]
    fn adversarial_expressions_never_panic() {
        let repeated = "a".repeat(10_000);
        let deep_generics = format!("{}int{}", "array<".repeat(200), ">".repeat(200));
        let deep_shapes = format!("{}int{}", "array{a:".repeat(200), "}".repeat(200));
        let deep_callables = format!("{}int{}", "callable(".repeat(200), "): int".repeat(200));
        let comment_bomb = "array{".to_owned() + &"// bomb\n".repeat(5_000) + "}";
        for text in [
            "????",
            "(((((",
            "]][[",
            "\u{0}|\u{0}",
            "&&&",
            "'unterminated",
            "\"unterminated",
            "Foo::",
            "$",
            "T is ? :",
            "int<",
            "array{...<",
            "callable():",
            "-",
            "...",
            "::",
            "a::*b::*c",
            deep_generics.as_str(),
            deep_shapes.as_str(),
            deep_callables.as_str(),
            comment_bomb.as_str(),
            repeated.as_str(),
        ] {
            let _ = parse_type_expression_text(text);
            let _ = parse_type_expression_prefix(text);
        }
    }
}
