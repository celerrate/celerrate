//! Declared types: lowering written native type text into the lattice
//! at a declaring site. The keyword table is total over the native
//! grammar; unknown names lower to class types (the judgment layer
//! answers `CannotProve` for unresolvable classes). Bare `callable`
//! lowers to `mixed`: a documented sound widening (no top-of-callables
//! form exists in the lattice; recorded debt, revisited by plan 8).

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    AstId, ClassQuery, LinearizedClass, Member, MemberKind, MemberQuery, SymbolQuery, SymbolSpace,
    UseTables, analyzed_file_index, folded_member_key, item_tree, linearized_class,
    lookup_class_declaration, lookup_function_declaration, lookup_member, member_tree,
    resolve_candidates,
};
use celerrate_stubs::StubIndexInput;

use crate::representation::TypeId;
use crate::written::{WrittenType, parse_written};

/// Where a written name qualifies: a source declaring site (namespace
/// plus `use` tables) or the global context (stub type texts).
pub(crate) enum NameSite<'a> {
    Source {
        namespace: &'a str,
        tables: &'a UseTables,
    },
    // Not constructed yet: the stub arms of tasks 10-11 build sites over
    // global (stub) type texts. Kept in the grammar so `lower_name`
    // already matches exhaustively over both sites.
    #[allow(dead_code)]
    Global,
}

pub(crate) fn lower_written_text<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    text: &str,
) -> Option<TypeId<'db>> {
    Some(lower_written(db, site, &parse_written(text)?))
}

pub(crate) fn lower_written<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    written: &WrittenType,
) -> TypeId<'db> {
    match written {
        WrittenType::Nullable(inner) => {
            TypeId::union(db, [lower_written(db, site, inner), TypeId::null(db)])
        }
        WrittenType::Union(parts) => {
            TypeId::union(db, parts.iter().map(|part| lower_written(db, site, part)))
        }
        WrittenType::Intersection(parts) => {
            TypeId::intersection(db, parts.iter().map(|part| lower_written(db, site, part)))
        }
        WrittenType::Name(name) => lower_name(db, site, name),
    }
}

#[allow(clippy::collapsible_if)]
fn lower_name<'db>(db: &'db dyn salsa::Database, site: &NameSite<'_>, name: &str) -> TypeId<'db> {
    if !name.contains('\\') {
        if let Some(keyword) = lower_keyword(db, name) {
            return keyword;
        }
    }
    TypeId::class(db, &qualified_class_name(site, name), vec![])
}

/// The keyword table: total over the native grammar (decision 3 for
/// `callable`). `None` means "an ordinary class name".
fn lower_keyword<'db>(db: &'db dyn salsa::Database, name: &str) -> Option<TypeId<'db>> {
    let folded = name.to_ascii_lowercase();
    Some(match folded.as_str() {
        "int" => TypeId::int(db),
        "float" => TypeId::float(db),
        "string" => TypeId::string(db),
        "bool" => TypeId::bool(db),
        "true" => TypeId::bool_literal(db, true),
        "false" => TypeId::bool_literal(db, false),
        "null" => TypeId::null(db),
        "mixed" => TypeId::mixed(db),
        "never" => TypeId::never(db),
        "void" => TypeId::void(db),
        "object" => TypeId::object(db),
        "resource" => TypeId::resource(db),
        "array" => TypeId::array(
            db,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
            TypeId::mixed(db),
        ),
        "iterable" => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        // Decision 3: no top-of-callables form exists; `mixed` is the
        // documented sound widening (recorded debt for plan 8).
        "callable" => TypeId::mixed(db),
        "self" => TypeId::self_placeholder(db),
        "static" => TypeId::static_placeholder(db),
        "parent" => TypeId::parent_placeholder(db),
        _ => return None,
    })
}

/// PHP class-name resolution is static: the first candidate is the
/// fully qualified name whether or not the class exists.
fn qualified_class_name(site: &NameSite<'_>, written: &str) -> String {
    match site {
        NameSite::Source { namespace, tables } => {
            resolve_candidates(written, SymbolSpace::ClassLike, namespace, tables)
                .into_iter()
                .next()
                .unwrap_or_else(|| written.trim_start_matches('\\').to_owned())
        }
        NameSite::Global => written.trim_start_matches('\\').to_owned(),
    }
}

/// How a declared element's final type was obtained — the trace the
/// design requires for annotation refinement (tasks 5-6 set the
/// non-native variants; the ground-truth harness of plan 6 reads it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum Trust {
    /// No annotation: the native declaration (or `mixed`) stands.
    NativeOnly,
    /// The annotation refines the native declaration (subtype: Holds).
    Refined,
    /// The annotation refines through an unproven judgment
    /// (CannotProve — template types, principally): trusted, traced.
    RefinedUnproven,
    /// The annotation contradicts the native declaration (Fails):
    /// ignored, the native declaration wins.
    RejectedAnnotation,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct DeclaredParameter<'db> {
    pub name: String,
    /// `None` silences every check on this parameter (the stub range
    /// rule's degenerate case, decision 6). An untyped parameter is
    /// `Some(mixed)`, never `None`.
    pub parameter_type: Option<TypeId<'db>>,
    pub trust: Trust,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct DeclaredSignature<'db> {
    /// Methods and functions; empty for properties, constants, cases.
    pub parameters: Vec<DeclaredParameter<'db>>,
    /// The return type (methods, functions), the property type, the
    /// constant type, or the enum-case type.
    pub value_type: TypeId<'db>,
    pub value_trust: Trust,
    pub by_reference: bool,
}

/// The parsed annotation layer of one member. Plan 4a's bridge fills
/// this through the type-syntax registry; until then every member
/// answers the default (no annotations). The seam is a tracked query
/// so the bridge swap changes ONE body and no signatures.
#[derive(Debug, Clone, Default, PartialEq, Eq, salsa::Update)]
pub struct MemberAnnotations<'db> {
    /// `@return` / `@var`: the annotated value type.
    pub value: Option<TypeId<'db>>,
    /// `@param`: annotated parameter types by parameter name.
    pub parameters: Vec<(String, TypeId<'db>)>,
}

#[salsa::tracked]
pub fn member_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> MemberAnnotations<'db> {
    // The seam: plan 4a's bridge replaces this body with the
    // docblock parse through the type-syntax registry. Everything
    // downstream (precedence, trust, inheritance) is already wired.
    let _ = (db, files, stubs, configuration, query);
    MemberAnnotations::default()
}

/// The source-precedence rule of the design's section 3: an
/// annotation refines the native declaration under the three-valued
/// judgment. Holds refines; Fails is ignored (native wins);
/// CannotProve refines and is traced. Never a crash, never a silent
/// widening, never a silently dropped annotation.
pub(crate) fn refine<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    native: TypeId<'db>,
    annotation: Option<TypeId<'db>>,
) -> (TypeId<'db>, Trust) {
    let Some(annotated) = annotation else {
        return (native, Trust::NativeOnly);
    };
    match crate::judgments::subtype_of(db, files, stubs, configuration, annotated, native) {
        crate::judgments::Proof::Holds => (annotated, Trust::Refined),
        crate::judgments::Proof::CannotProve => (annotated, Trust::RefinedUnproven),
        crate::judgments::Proof::Fails => (native, Trust::RejectedAnnotation),
    }
}

/// The ancestor keys of one linearized class, nearest first (edge
/// walk order), the root and duplicates removed. Stub ancestors take
/// no part: annotations live in source docblocks.
fn ancestors_in_walk_order(root_key: &str, linearized: &LinearizedClass) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for edge in &linearized.ancestry {
        let Some(resolved) = edge.resolved.as_deref() else {
            continue;
        };
        if resolved == root_key || seen.iter().any(|key| key == resolved) {
            continue;
        }
        seen.push(resolved.to_owned());
    }
    seen
}

/// Element-wise nearest-ancestor merge: own annotations first, then
/// each declaring ancestor in walk order fills only what is still
/// missing. `declares` gates on the ancestor's OWN member table (an
/// ancestor that merely inherits the member is not its annotation
/// site); `read` supplies the ancestor's parsed annotations.
fn inherited_annotations<'db>(
    own: MemberAnnotations<'db>,
    parameter_names: &[String],
    ancestors: &[String],
    declares: impl Fn(&str) -> bool,
    read: impl Fn(&str) -> MemberAnnotations<'db>,
) -> MemberAnnotations<'db> {
    let mut merged = own;
    for ancestor in ancestors {
        let value_missing = merged.value.is_none();
        let missing_parameters: Vec<&String> = parameter_names
            .iter()
            .filter(|name| {
                !merged
                    .parameters
                    .iter()
                    .any(|(merged_name, _)| merged_name == *name)
            })
            .collect();
        if !value_missing && missing_parameters.is_empty() {
            return merged;
        }
        if !declares(ancestor) {
            continue;
        }
        let ancestor_annotations = read(ancestor);
        if value_missing {
            merged.value = ancestor_annotations.value;
        }
        for name in missing_parameters {
            if let Some((_, annotated)) = ancestor_annotations
                .parameters
                .iter()
                .find(|(ancestor_name, _)| ancestor_name == name)
            {
                merged.parameters.push((name.clone(), *annotated));
            }
        }
    }
    merged
}

#[salsa::tracked]
pub fn declared_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<DeclaredSignature<'db>> {
    let resolution = lookup_member(db, files, stubs, configuration, query)?;
    let site_parts = declaring_site(db, files, &resolution.owner)?;
    let tables = UseTables::for_namespace(item_tree(db, site_parts.file), &site_parts.namespace);
    let site = NameSite::Source {
        namespace: &site_parts.namespace,
        tables: &tables,
    };
    let own = member_annotations(db, files, stubs, configuration, query);
    let root_key = query.class_key(db);
    let class = ClassQuery::new(db, root_key.clone());
    let linearized = linearized_class(db, files, stubs, configuration, class);
    let parameter_names: Vec<String> = resolution
        .member
        .signature
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    let annotations = match linearized.as_ref() {
        Some(linearized) => {
            let ancestors = ancestors_in_walk_order(root_key, linearized);
            let kind = query.kind(db);
            let member_key = query.member_key(db);
            inherited_annotations(
                own,
                &parameter_names,
                &ancestors,
                |ancestor| declares_member(db, files, ancestor, kind, member_key),
                |ancestor| {
                    let ancestor_query =
                        MemberQuery::new(db, ancestor.to_owned(), kind, member_key.clone());
                    member_annotations(db, files, stubs, configuration, ancestor_query)
                },
            )
        }
        None => own,
    };
    Some(resolve_member_signature(
        db,
        files,
        stubs,
        configuration,
        &site,
        &resolution.owner,
        &resolution.member,
        &annotations,
    ))
}

/// The declaring site of one source class-like: its file handle,
/// namespace, and declaration AST id, found through the same
/// firewalls linearization uses.
struct DeclaringSite {
    file: celerrate_db::SourceFile,
    namespace: String,
    ast_id: AstId,
}

fn declaring_site(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    owner_key: &str,
) -> Option<DeclaringSite> {
    let query = SymbolQuery::new(db, SymbolSpace::ClassLike, owner_key.to_owned());
    let (_, ast_id) = lookup_class_declaration(db, files, query)?;
    let index = analyzed_file_index(db, files);
    let position = index
        .binary_search_by_key(&ast_id.file, |(id, _)| *id)
        .ok()?;
    let (_, file) = *index.get(position)?;
    let namespace = member_tree(db, file)
        .classes
        .iter()
        .find(|group| group.ast_id == ast_id)?
        .namespace
        .clone();
    Some(DeclaringSite {
        file,
        namespace,
        ast_id,
    })
}

/// Whether `class_key`'s OWN member group declares a member of this
/// kind and key (inherited entries do not count: the annotation site
/// is the declaring docblock).
fn declares_member(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    class_key: &str,
    kind: MemberKind,
    member_key: &str,
) -> bool {
    let Some(site) = declaring_site(db, files, class_key) else {
        return false;
    };
    member_tree(db, site.file)
        .classes
        .iter()
        .find(|group| group.ast_id == site.ast_id)
        .is_some_and(|group| {
            group.members.iter().any(|member| {
                member.kind == kind && folded_member_key(kind, &member.name) == member_key
            })
        })
}

/// Resolves one member's written signature at its declaring site.
/// Own annotations refine the native declaration through [`refine`]
/// (task 5); the inheritance walk that folds in ancestor annotations
/// lands in task 6. Enum cases skip refinement: their type is their
/// identity, never annotated.
#[allow(clippy::too_many_arguments)]
fn resolve_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    site: &NameSite<'_>,
    owner_key: &str,
    member: &Member,
    annotations: &MemberAnnotations<'db>,
) -> DeclaredSignature<'db> {
    let (value_type, value_trust) = match member.kind {
        MemberKind::EnumCase => (
            TypeId::enum_case(db, owner_key, &member.name),
            Trust::NativeOnly,
        ),
        // Task 10 reshapes `MemberResolution` into an enum; until then
        // this stays the current struct, destructured as such.
        MemberKind::ClassConstant => {
            let native = match member.signature.type_text.as_deref() {
                Some(text) => lowered_or_mixed(db, site, Some(text)),
                None => member
                    .signature
                    .default_text
                    .as_deref()
                    .and_then(|text| literal_type_of_default(db, text))
                    .unwrap_or_else(|| TypeId::mixed(db)),
            };
            refine(db, files, stubs, configuration, native, annotations.value)
        }
        MemberKind::Method | MemberKind::Property => {
            let native = lowered_or_mixed(db, site, member.signature.type_text.as_deref());
            refine(db, files, stubs, configuration, native, annotations.value)
        }
    };
    DeclaredSignature {
        parameters: member
            .signature
            .parameters
            .iter()
            .map(|parameter| {
                let annotation = annotations
                    .parameters
                    .iter()
                    .find(|(name, _)| *name == parameter.name)
                    .map(|(_, annotated)| *annotated);
                declared_parameter(db, files, stubs, configuration, site, parameter, annotation)
            })
            .collect(),
        value_type,
        value_trust,
        by_reference: member.signature.by_reference,
    }
}

#[allow(clippy::too_many_arguments)]
fn declared_parameter<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    site: &NameSite<'_>,
    parameter: &celerrate_semantics::ParameterSignature,
    annotation: Option<TypeId<'db>>,
) -> DeclaredParameter<'db> {
    let mut native_parameter_type = lowered_or_mixed(db, site, parameter.type_text.as_deref());
    // Implicit nullability (design section 2): `Type $x = null`.
    if parameter
        .default_text
        .as_deref()
        .is_some_and(|text| text.eq_ignore_ascii_case("null"))
    {
        native_parameter_type = TypeId::union(db, [native_parameter_type, TypeId::null(db)]);
    }
    let (parameter_type, trust) = refine(
        db,
        files,
        stubs,
        configuration,
        native_parameter_type,
        annotation,
    );
    DeclaredParameter {
        name: parameter.name.clone(),
        parameter_type: Some(parameter_type),
        trust,
        optional: parameter.default_text.is_some() || parameter.variadic,
        variadic: parameter.variadic,
        by_reference: parameter.by_reference,
    }
}

/// Written text to lattice type: absent or malformed text is `mixed`
/// (resilience: a signature the parser mangled must never error).
fn lowered_or_mixed<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    text: Option<&str>,
) -> TypeId<'db> {
    text.and_then(|text| lower_written_text(db, site, text))
        .unwrap_or_else(|| TypeId::mixed(db))
}

/// The literal type of a comparable default text (`expression_text`
/// form: tokens joined with one space): integers (optionally `- `
/// prefixed), floats, single-quoted strings, `true`/`false`/`null`.
/// Anything else — expressions, constants, arrays — is `None`.
fn literal_type_of_default<'db>(db: &'db dyn salsa::Database, text: &str) -> Option<TypeId<'db>> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Some(TypeId::bool_literal(db, true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Some(TypeId::bool_literal(db, false));
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Some(TypeId::null(db));
    }
    if let Some(unquoted) = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        // Single-quoted with no escapes: the raw content is the value.
        if !unquoted.contains('\\') && !unquoted.contains('\'') {
            return Some(TypeId::string_literal(db, unquoted));
        }
        return None;
    }
    let (negative, digits) = match trimmed.strip_prefix("- ") {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    if digits.bytes().all(|byte| byte.is_ascii_digit()) && !digits.is_empty() {
        // Leading-zero forms (e.g., 017) are octal in PHP; refuse them rather
        // than mis-value as base-10. A bare "0" stays a valid literal.
        if digits.len() > 1 && digits.starts_with('0') {
            return None;
        }
        let value = digits.parse::<i64>().ok()?;
        return Some(TypeId::int_literal(
            db,
            if negative { -value } else { value },
        ));
    }
    if let Ok(value) = digits.parse::<f64>()
        && digits.contains('.')
    {
        return Some(TypeId::float_literal(
            db,
            if negative { -value } else { value },
        ));
    }
    None
}

#[salsa::interned(debug)]
pub struct FunctionQuery<'db> {
    /// Pre-folded Function-space key.
    #[returns(ref)]
    pub key: String,
}

#[salsa::tracked]
pub fn declared_function_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> Option<DeclaredSignature<'db>> {
    let symbol_query = SymbolQuery::new(db, SymbolSpace::Function, query.key(db).clone());
    let ast_id = lookup_function_declaration(db, files, symbol_query)?;
    let index = analyzed_file_index(db, files);
    let position = index
        .binary_search_by_key(&ast_id.file, |(id, _)| *id)
        .ok()?;
    let (_, file) = *index.get(position)?;
    let function = member_tree(db, file)
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)?
        .clone();
    let tables = UseTables::for_namespace(item_tree(db, file), &function.namespace);
    let site = NameSite::Source {
        namespace: &function.namespace,
        tables: &tables,
    };
    Some(DeclaredSignature {
        parameters: function
            .signature
            .parameters
            .iter()
            .map(|parameter| {
                declared_parameter(db, files, stubs, configuration, &site, parameter, None)
            })
            .collect(),
        value_type: lowered_or_mixed(db, &site, function.signature.type_text.as_deref()),
        value_trust: Trust::NativeOnly,
        by_reference: function.signature.by_reference,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;

    use super::{NameSite, lower_written_text};
    use crate::representation::TypeId;

    fn lower<'db>(db: &'db TestDatabase, text: &str) -> Option<TypeId<'db>> {
        lower_written_text(db, &NameSite::Global, text)
    }

    #[test]
    fn the_keyword_table_is_total_over_the_native_grammar() {
        let db = TestDatabase::default();
        let cases: &[(&str, TypeId<'_>)] = &[
            ("int", TypeId::int(&db)),
            ("INT", TypeId::int(&db)),
            ("float", TypeId::float(&db)),
            ("string", TypeId::string(&db)),
            ("bool", TypeId::bool(&db)),
            ("true", TypeId::bool_literal(&db, true)),
            ("false", TypeId::bool_literal(&db, false)),
            ("null", TypeId::null(&db)),
            ("mixed", TypeId::mixed(&db)),
            ("never", TypeId::never(&db)),
            ("void", TypeId::void(&db)),
            ("object", TypeId::object(&db)),
            ("resource", TypeId::resource(&db)),
            ("self", TypeId::self_placeholder(&db)),
            ("static", TypeId::static_placeholder(&db)),
            ("parent", TypeId::parent_placeholder(&db)),
        ];
        for (text, expected) in cases {
            assert_eq!(lower(&db, text), Some(*expected), "keyword {text}");
        }
    }

    #[test]
    fn array_iterable_and_callable_lower_to_their_documented_forms() {
        let db = TestDatabase::default();
        let array_key = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        assert_eq!(
            lower(&db, "array"),
            Some(TypeId::array(&db, array_key, TypeId::mixed(&db))),
        );
        assert_eq!(
            lower(&db, "iterable"),
            Some(TypeId::iterable(
                &db,
                TypeId::mixed(&db),
                TypeId::mixed(&db)
            )),
        );
        // Decision 3: bare `callable` is a documented sound widening.
        assert_eq!(lower(&db, "callable"), Some(TypeId::mixed(&db)));
    }

    #[test]
    fn nullable_union_and_intersection_lower_through_the_lattice() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "?int"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])),
        );
        assert_eq!(
            lower(&db, "int|string"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)])),
        );
        let countable = TypeId::class(&db, "Countable", vec![]);
        let iterator = TypeId::class(&db, "Iterator", vec![]);
        assert_eq!(
            lower(&db, "Countable&Iterator"),
            Some(TypeId::intersection(&db, [countable, iterator])),
        );
    }

    #[test]
    fn global_names_lower_to_class_types_with_the_backslash_trimmed() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "\\DateTime"),
            Some(TypeId::class(&db, "DateTime", vec![])),
        );
        // A qualified name is never a keyword.
        assert_eq!(
            lower(&db, "Foo\\int"),
            Some(TypeId::class(&db, "Foo\\int", vec![])),
        );
    }

    #[test]
    fn source_site_names_qualify_through_namespace_and_imports() {
        use celerrate_db::{AnalyzedFileSet, SourceFile};
        use celerrate_semantics::{UseTables, item_tree};
        use celerrate_source::FileId;

        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace App; use Psr\\Log\\LoggerInterface as Logger; class C {}".to_vec(),
        );
        let _files = AnalyzedFileSet::new(&db, vec![file]);
        let tree = item_tree(&db, file);
        let tables = UseTables::for_namespace(tree, "App");
        let site = super::NameSite::Source {
            namespace: "App",
            tables: &tables,
        };
        // The import expands.
        assert_eq!(
            lower_written_text(&db, &site, "Logger"),
            Some(TypeId::class(&db, "Psr\\Log\\LoggerInterface", vec![])),
        );
        // An unimported name qualifies into the namespace, existing or not.
        assert_eq!(
            lower_written_text(&db, &site, "Repository"),
            Some(TypeId::class(&db, "App\\Repository", vec![])),
        );
        // Absolute names ignore the namespace.
        assert_eq!(
            lower_written_text(&db, &site, "\\Throwable"),
            Some(TypeId::class(&db, "Throwable", vec![])),
        );
    }

    #[test]
    fn malformed_text_lowers_to_none() {
        let db = TestDatabase::default();
        assert_eq!(lower(&db, ""), None);
        assert_eq!(lower(&db, "A|"), None);
    }

    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{
        MemberKind, MemberQuery, SymbolSpace, folded_member_key, folded_symbol_key,
    };
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{
        DeclaredSignature, FunctionQuery, Trust, declared_function_signature,
        declared_member_signature,
    };

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn member<'db>(
        fixture: &'db Fixture,
        class_written: &str,
        kind: MemberKind,
        member_written: &str,
    ) -> Option<DeclaredSignature<'db>> {
        let query = MemberQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, class_written),
            kind,
            folded_member_key(kind, member_written),
        );
        declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    #[test]
    fn a_method_signature_resolves_at_its_declaring_site() {
        let fixture = fixture(&["<?php namespace App;\n\
             use Psr\\Log\\LoggerInterface as Logger;\n\
             class Service {\n\
                 public function handle(Logger $logger, int $count = 3): ?string {}\n\
             }"]);
        let signature = member(&fixture, "App\\Service", MemberKind::Method, "handle").unwrap();
        let db = &fixture.db;
        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::class(db, "Psr\\Log\\LoggerInterface", vec![])),
        );
        assert!(!signature.parameters[0].optional);
        assert_eq!(
            signature.parameters[1].parameter_type,
            Some(TypeId::int(db))
        );
        assert!(signature.parameters[1].optional);
        assert_eq!(
            signature.value_type,
            TypeId::union(db, [TypeId::string(db), TypeId::null(db)]),
        );
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn an_untyped_parameter_is_mixed_and_a_null_default_makes_it_nullable() {
        let fixture = fixture(&[
            "<?php class C { public function f($anything, ?Logger $log = null, Widget $w = null) {} }",
        ]);
        let signature = member(&fixture, "C", MemberKind::Method, "f").unwrap();
        let db = &fixture.db;
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::mixed(db))
        );
        // `= null` on an already-nullable type changes nothing.
        let nullable_logger =
            TypeId::union(db, [TypeId::class(db, "Logger", vec![]), TypeId::null(db)]);
        assert_eq!(
            signature.parameters[1].parameter_type,
            Some(nullable_logger)
        );
        // Implicit nullability: `Widget $w = null` admits null.
        let nullable_widget =
            TypeId::union(db, [TypeId::class(db, "Widget", vec![]), TypeId::null(db)]);
        assert_eq!(
            signature.parameters[2].parameter_type,
            Some(nullable_widget)
        );
        // No declared return: mixed.
        assert_eq!(signature.value_type, TypeId::mixed(db));
    }

    #[test]
    fn properties_constants_and_enum_cases_declare_their_value_types() {
        let fixture = fixture(&["<?php\n\
             class C {\n\
                 public ?int $count;\n\
                 public $untyped;\n\
                 const ACTIVE = 'active';\n\
                 const int LIMIT = 10;\n\
             }\n\
             enum Status: string { case Active = 'active'; }"]);
        let db = &fixture.db;
        let count = member(&fixture, "C", MemberKind::Property, "count").unwrap();
        assert_eq!(
            count.value_type,
            TypeId::union(db, [TypeId::int(db), TypeId::null(db)]),
        );
        let untyped = member(&fixture, "C", MemberKind::Property, "untyped").unwrap();
        assert_eq!(untyped.value_type, TypeId::mixed(db));
        // An untyped constant with a literal default carries the literal.
        let active = member(&fixture, "C", MemberKind::ClassConstant, "ACTIVE").unwrap();
        assert_eq!(active.value_type, TypeId::string_literal(db, "active"));
        // A typed constant (8.3) uses its written type.
        let limit = member(&fixture, "C", MemberKind::ClassConstant, "LIMIT").unwrap();
        assert_eq!(limit.value_type, TypeId::int(db));
        let case = member(&fixture, "Status", MemberKind::EnumCase, "Active").unwrap();
        assert_eq!(case.value_type, TypeId::enum_case(db, "Status", "Active"));
    }

    #[test]
    fn leading_zero_integer_defaults_refuse_octal_misvaluing() {
        let fixture = fixture(&["<?php\n\
             class C {\n\
                 const LEGACY = 017;\n\
                 const ZERO = 0;\n\
             }"]);
        let db = &fixture.db;
        // Leading-zero literals are octal in PHP and are refused rather than
        // mis-valued as base-10.
        let legacy = member(&fixture, "C", MemberKind::ClassConstant, "LEGACY").unwrap();
        assert_eq!(legacy.value_type, TypeId::mixed(db));
        // A bare 0 is still a valid int literal.
        let zero = member(&fixture, "C", MemberKind::ClassConstant, "ZERO").unwrap();
        assert_eq!(zero.value_type, TypeId::int_literal(db, 0));
    }

    #[test]
    fn an_inherited_member_resolves_in_the_declaring_class_namespace() {
        let fixture = fixture(&[
            "<?php namespace Lib; class Base { public function make(): Widget {} }",
            "<?php namespace App; class Child extends \\Lib\\Base {}",
        ]);
        let signature = member(&fixture, "App\\Child", MemberKind::Method, "make").unwrap();
        // `Widget` qualifies in Lib (the declaring site), never in App.
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "Lib\\Widget", vec![]),
        );
    }

    #[test]
    fn a_free_function_signature_resolves_like_a_method() {
        let fixture = fixture(&["<?php namespace App; function build(int $count): ?Widget {}"]);
        let query = FunctionQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::Function, "App\\build"),
        );
        let signature = declared_function_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        let db = &fixture.db;
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::int(db))
        );
        assert_eq!(
            signature.value_type,
            TypeId::union(
                db,
                [TypeId::class(db, "App\\Widget", vec![]), TypeId::null(db)]
            ),
        );
    }

    #[test]
    fn unknown_members_and_malformed_types_degrade_cleanly() {
        let fixture = fixture(&["<?php class C { public function f(): int {} }"]);
        assert!(member(&fixture, "C", MemberKind::Method, "ghost").is_none());
        assert!(member(&fixture, "Ghost", MemberKind::Method, "f").is_none());
    }

    #[test]
    fn the_trust_rule_is_three_valued() {
        let fixture = fixture(&["<?php interface Animal {} class Dog implements Animal {}"]);
        let db = &fixture.db;
        let animal = TypeId::class(db, "Animal", vec![]);
        let dog = TypeId::class(db, "Dog", vec![]);
        let int = TypeId::int(db);
        let refine = |native, annotation| {
            super::refine(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                native,
                annotation,
            )
        };
        // No annotation: native stands.
        assert_eq!(refine(animal, None), (animal, Trust::NativeOnly));
        // Holds: the annotation refines.
        assert_eq!(refine(animal, Some(dog)), (dog, Trust::Refined));
        // Fails: the annotation is ignored, the native declaration wins.
        assert_eq!(refine(int, Some(animal)), (int, Trust::RejectedAnnotation));
        // CannotProve (an unresolvable class): refines, traced.
        let ghost = TypeId::class(db, "Ghost", vec![]);
        assert_eq!(refine(animal, Some(ghost)), (ghost, Trust::RefinedUnproven),);
    }

    #[test]
    fn the_annotation_seam_answers_the_default_until_the_bridge_lands() {
        let fixture = fixture(&["<?php class C { /** @return int */ public function f() {} }"]);
        let query = MemberQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, "C"),
            MemberKind::Method,
            folded_member_key(MemberKind::Method, "f"),
        );
        let annotations = super::member_annotations(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        );
        assert_eq!(annotations, super::MemberAnnotations::default());
    }

    #[test]
    fn the_nearest_ancestor_annotation_wins_element_wise() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        let bool_type = TypeId::bool(&db);
        let annotations_of = |key: &str| -> super::MemberAnnotations<'_> {
            match key {
                // The near ancestor annotates only the parameter.
                "near" => super::MemberAnnotations {
                    value: None,
                    parameters: vec![("x".to_owned(), string)],
                },
                // The far ancestor annotates both.
                "far" => super::MemberAnnotations {
                    value: Some(int),
                    parameters: vec![("x".to_owned(), bool_type)],
                },
                _ => super::MemberAnnotations::default(),
            }
        };
        let ancestors = vec!["near".to_owned(), "far".to_owned()];
        let merged = super::inherited_annotations(
            super::MemberAnnotations::default(),
            &["x".to_owned()],
            &ancestors,
            |_| true,
            annotations_of,
        );
        // Value: the near ancestor is silent, the far one supplies it.
        assert_eq!(merged.value, Some(int));
        // Parameter: the near ancestor wins over the far one.
        assert_eq!(merged.parameters, vec![("x".to_owned(), string)]);
    }

    #[test]
    fn own_annotations_shadow_every_ancestor_and_non_declaring_ancestors_are_skipped() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        let own = super::MemberAnnotations {
            value: Some(int),
            parameters: vec![],
        };
        let merged = super::inherited_annotations(
            own.clone(),
            &[],
            &["ancestor".to_owned()],
            |_| true,
            |_| super::MemberAnnotations {
                value: Some(string),
                parameters: vec![],
            },
        );
        assert_eq!(merged.value, Some(int), "own annotation shadows");

        // An ancestor that does not declare the member supplies nothing.
        let merged = super::inherited_annotations(
            super::MemberAnnotations::default(),
            &[],
            &["silent".to_owned()],
            |_| false,
            |_| super::MemberAnnotations {
                value: Some(string),
                parameters: vec![],
            },
        );
        assert_eq!(merged.value, None);
    }

    #[test]
    fn ancestors_walk_in_linearization_order_without_the_root_or_duplicates() {
        let fixture = fixture(&["<?php\n\
             interface I {}\n\
             class A implements I {}\n\
             class B extends A implements I {}\n\
             class C extends B {}"]);
        let key = folded_symbol_key(SymbolSpace::ClassLike, "C");
        let class = celerrate_semantics::ClassQuery::new(&fixture.db, key.clone());
        let linearized = celerrate_semantics::linearized_class(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            class,
        )
        .as_ref()
        .unwrap();
        assert_eq!(
            super::ancestors_in_walk_order(&key, linearized),
            vec!["b".to_owned(), "a".to_owned(), "i".to_owned()],
        );
    }

    #[test]
    fn inheritance_wiring_leaves_native_results_untouched_while_the_seam_is_empty() {
        let fixture = fixture(&[
            "<?php interface Normalizer { public function normalize($data): array {} }\n\
             class UserNormalizer implements Normalizer {\n\
                 public function normalize($data): array {}\n\
             }",
        ]);
        let signature =
            member(&fixture, "UserNormalizer", MemberKind::Method, "normalize").unwrap();
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }
}
