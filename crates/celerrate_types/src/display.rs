//! Deterministic rendering: PHPStan-flavored spellings over the
//! canonical structure. Class names render as their folded keys in this
//! plan (recorded debt: plan 8 recovers original spellings through the
//! symbol table when rendering diagnostics).

use crate::representation::{StringConstraint, TypeData, TypeId};

pub(crate) fn display_type<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> String {
    match of.data(db) {
        TypeData::Mixed => "mixed".to_owned(),
        TypeData::Never => "never".to_owned(),
        TypeData::Void => "void".to_owned(),
        TypeData::Null => "null".to_owned(),
        TypeData::Object => "object".to_owned(),
        TypeData::Resource => "resource".to_owned(),
        TypeData::Bool { literal: None } => "bool".to_owned(),
        TypeData::Bool {
            literal: Some(true),
        } => "true".to_owned(),
        TypeData::Bool {
            literal: Some(false),
        } => "false".to_owned(),
        TypeData::Int {
            minimum: None,
            maximum: None,
        } => "int".to_owned(),
        TypeData::Int {
            minimum: Some(low),
            maximum: Some(high),
        } if low == high => low.to_string(),
        TypeData::Int { minimum, maximum } => format!(
            "int<{}, {}>",
            minimum.map_or_else(|| "min".to_owned(), |bound| bound.to_string()),
            maximum.map_or_else(|| "max".to_owned(), |bound| bound.to_string()),
        ),
        TypeData::Float { literal: None } => "float".to_owned(),
        TypeData::Float {
            literal: Some(bits),
        } => {
            let rendered = format!("{}", bits.value());
            if rendered.contains(['.', 'e', 'E'])
                || rendered.contains("inf")
                || rendered.contains("NaN")
            {
                rendered
            } else {
                format!("{rendered}.0")
            }
        }
        TypeData::String {
            constraint: StringConstraint::General,
        } => "string".to_owned(),
        TypeData::String {
            constraint: StringConstraint::NonEmpty,
        } => "non-empty-string".to_owned(),
        TypeData::String {
            constraint: StringConstraint::Numeric,
        } => "numeric-string".to_owned(),
        TypeData::String {
            constraint: StringConstraint::LiteralMarker,
        } => "literal-string".to_owned(),
        TypeData::String {
            constraint: StringConstraint::Literal(value),
        } => format!("'{value}'"),
        TypeData::ClassString { argument: None } => "class-string".to_owned(),
        TypeData::ClassString {
            argument: Some(argument),
        } => {
            format!("class-string<{}>", display_type(db, *argument))
        }
        TypeData::Union { constituents } => {
            // Render null last: `User|null`, the conventional spelling.
            let (null_parts, other_parts): (Vec<_>, Vec<_>) =
                constituents.iter().partition(|part| part.is_null(db));
            let mut rendered: Vec<String> = other_parts
                .iter()
                .map(|&&part| parenthesized(db, part))
                .collect();
            rendered.extend(null_parts.iter().map(|&&part| display_type(db, part)));
            rendered.join("|")
        }
        TypeData::Intersection { intersectands } => intersectands
            .iter()
            .map(|part| parenthesized(db, *part))
            .collect::<Vec<_>>()
            .join("&"),
        TypeData::Array {
            key,
            value,
            is_list,
            non_empty,
        } => match (is_list, non_empty) {
            (true, true) => format!("non-empty-list<{}>", display_type(db, *value)),
            (true, false) => format!("list<{}>", display_type(db, *value)),
            (false, true) => format!(
                "non-empty-array<{}, {}>",
                display_type(db, *key),
                display_type(db, *value)
            ),
            (false, false) => {
                format!(
                    "array<{}, {}>",
                    display_type(db, *key),
                    display_type(db, *value)
                )
            }
        },
        TypeData::Shape { fields } => {
            let rendered: Vec<String> = fields
                .iter()
                .map(|field| {
                    let key = match &field.key {
                        crate::ShapeKey::Integer(value) => value.to_string(),
                        crate::ShapeKey::String(value) => value.clone(),
                    };
                    let marker = if field.optional { "?" } else { "" };
                    format!("{key}{marker}: {}", display_type(db, field.value))
                })
                .collect();
            format!("array{{{}}}", rendered.join(", "))
        }
        TypeData::Class { name, arguments } => {
            if arguments.is_empty() {
                name.clone()
            } else {
                let rendered: Vec<String> = arguments
                    .iter()
                    .map(|argument| display_type(db, *argument))
                    .collect();
                format!("{name}<{}>", rendered.join(", "))
            }
        }
        TypeData::EnumCase {
            enum_name,
            case_name,
        } => format!("{enum_name}::{case_name}"),
        TypeData::Callable {
            parameters,
            return_type,
        } => {
            let rendered: Vec<String> = parameters
                .iter()
                .map(|parameter| {
                    let mut text = display_type(db, parameter.parameter_type);
                    if parameter.by_reference {
                        text.push_str(" &");
                    }
                    if parameter.variadic {
                        text.push_str("...");
                    } else if parameter.optional {
                        text.push('=');
                    }
                    text
                })
                .collect();
            format!(
                "callable({}): {}",
                rendered.join(", "),
                display_type(db, *return_type)
            )
        }
        TypeData::Template { name, bound, .. } => {
            if bound.is_mixed(db) {
                name.clone()
            } else {
                format!("{name} of {}", display_type(db, *bound))
            }
        }
        TypeData::KeyOf { subject } => format!("key-of<{}>", display_type(db, *subject)),
        TypeData::ValueOf { subject } => format!("value-of<{}>", display_type(db, *subject)),
        TypeData::Conditional {
            subject,
            matches,
            then_branch,
            otherwise_branch,
            negated,
        } => {
            let operator = if *negated { "is not" } else { "is" };
            format!(
                "({} {operator} {} ? {} : {})",
                display_type(db, *subject),
                display_type(db, *matches),
                display_type(db, *then_branch),
                display_type(db, *otherwise_branch),
            )
        }
        TypeData::SelfPlaceholder => "self".to_owned(),
        TypeData::ParentPlaceholder => "parent".to_owned(),
        TypeData::StaticPlaceholder => "static".to_owned(),
    }
}

/// Unions and intersections nested inside another compound render in
/// parentheses.
fn parenthesized<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> String {
    let rendered = display_type(db, of);
    match of.data(db) {
        TypeData::Union { .. } | TypeData::Intersection { .. } => format!("({rendered})"),
        _ => rendered,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::{CallableParameter, ShapeField, ShapeKey, TypeId};

    #[test]
    fn atoms_and_literals_render() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::mixed(&db).display(&db), "mixed");
        assert_eq!(TypeId::never(&db).display(&db), "never");
        assert_eq!(TypeId::void(&db).display(&db), "void");
        assert_eq!(TypeId::null(&db).display(&db), "null");
        assert_eq!(TypeId::object(&db).display(&db), "object");
        assert_eq!(TypeId::resource(&db).display(&db), "resource");
        assert_eq!(TypeId::bool(&db).display(&db), "bool");
        assert_eq!(TypeId::bool_literal(&db, true).display(&db), "true");
        assert_eq!(TypeId::int(&db).display(&db), "int");
        assert_eq!(TypeId::int_literal(&db, 42).display(&db), "42");
        assert_eq!(
            TypeId::int_range(&db, Some(1), None).display(&db),
            "int<1, max>"
        );
        assert_eq!(
            TypeId::int_range(&db, None, Some(5)).display(&db),
            "int<min, 5>"
        );
        assert_eq!(TypeId::float(&db).display(&db), "float");
        assert_eq!(TypeId::float_literal(&db, 1.5).display(&db), "1.5");
        assert_eq!(TypeId::string(&db).display(&db), "string");
        assert_eq!(
            TypeId::non_empty_string(&db).display(&db),
            "non-empty-string"
        );
        assert_eq!(TypeId::numeric_string(&db).display(&db), "numeric-string");
        assert_eq!(
            TypeId::literal_string_type(&db).display(&db),
            "literal-string"
        );
        assert_eq!(
            TypeId::string_literal(&db, "active").display(&db),
            "'active'"
        );
    }

    #[test]
    fn composites_render_with_null_last_in_unions() {
        let db = TestDatabase::default();
        let nullable = TypeId::union(&db, [TypeId::null(&db), TypeId::class(&db, "User", vec![])]);
        assert_eq!(nullable.display(&db), "user|null");
        assert_eq!(
            TypeId::intersection(
                &db,
                [
                    TypeId::class(&db, "Foo", vec![]),
                    TypeId::class(&db, "Countable", vec![])
                ]
            )
            .display(&db),
            "countable&foo"
        );
        assert_eq!(
            TypeId::array(&db, TypeId::string(&db), TypeId::int(&db)).display(&db),
            "array<string, int>"
        );
        assert_eq!(
            TypeId::list(&db, TypeId::int(&db)).display(&db),
            "list<int>"
        );
        assert_eq!(
            TypeId::non_empty_array(&db, TypeId::string(&db), TypeId::int(&db)).display(&db),
            "non-empty-array<string, int>"
        );
        assert_eq!(
            TypeId::non_empty_list(&db, TypeId::int(&db)).display(&db),
            "non-empty-list<int>"
        );
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::String("name".to_owned()),
                    optional: true,
                    value: TypeId::string(&db),
                },
            ],
        );
        assert_eq!(shape.display(&db), "array{id: int, name?: string}");
        assert_eq!(
            TypeId::class(&db, "Collection", vec![TypeId::class(&db, "User", vec![])]).display(&db),
            "collection<user>"
        );
        assert_eq!(
            TypeId::enum_case(&db, "Status", "Active").display(&db),
            "status::Active"
        );
        assert_eq!(TypeId::class_string(&db, None).display(&db), "class-string");
        let callable = TypeId::callable(
            &db,
            vec![
                CallableParameter {
                    parameter_type: TypeId::int(&db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::string(&db),
                    optional: true,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::bool(&db),
                    optional: false,
                    variadic: true,
                    by_reference: false,
                },
            ],
            TypeId::void(&db),
        );
        assert_eq!(
            callable.display(&db),
            "callable(int, string=, bool...): void"
        );
        let template = TypeId::template(&db, "scope", "T", TypeId::class(&db, "Foo", vec![]));
        assert_eq!(template.display(&db), "T of foo");
        assert_eq!(
            TypeId::template(&db, "scope", "T", TypeId::mixed(&db)).display(&db),
            "T"
        );
        let symbolic = TypeId::key_of(&db, template);
        assert_eq!(symbolic.display(&db), "key-of<T of foo>");
        assert_eq!(TypeId::static_placeholder(&db).display(&db), "static");
        assert_eq!(TypeId::self_placeholder(&db).display(&db), "self");
        assert_eq!(TypeId::parent_placeholder(&db).display(&db), "parent");
    }

    #[test]
    fn nested_unions_inside_intersections_are_parenthesized() {
        let db = TestDatabase::default();
        let union = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        let intersection =
            TypeId::intersection(&db, [union, TypeId::class(&db, "Countable", vec![])]);
        assert_eq!(intersection.display(&db), "countable&(int|string)");
    }

    #[test]
    fn whole_number_float_literals_keep_their_decimal_point() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::float_literal(&db, 2.0).display(&db), "2.0");
        assert_eq!(TypeId::float_literal(&db, -2.0).display(&db), "-2.0");
        assert_eq!(TypeId::float_literal(&db, 1.5).display(&db), "1.5");
    }
}
