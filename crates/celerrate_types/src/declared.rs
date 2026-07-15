//! Declared types: lowering written native type text into the lattice
//! at a declaring site. The keyword table is total over the native
//! grammar; unknown names lower to class types (the judgment layer
//! answers `CannotProve` for unresolvable classes). Bare `callable`
//! lowers to `mixed`: a documented sound widening (no top-of-callables
//! form exists in the lattice; recorded debt, revisited by plan 8).

use celerrate_db::AnalyzedFileSet;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration, SUPPORTED_VERSIONS};
use celerrate_semantics::{
    AstId, ClassQuery, LinearizedClass, Member, MemberKind, MemberQuery, MemberResolution,
    SymbolQuery, SymbolSpace, UseTables, analyzed_file_index, folded_member_key, item_tree,
    linearized_class, lookup_class_declaration, lookup_function_declaration, lookup_member,
    member_tree, resolve_candidates, stub_signature_table,
};
use celerrate_stubs::{
    StubIndexInput, StubMember, StubMemberKind, StubParameter, StubSignature, VersionedTypeText,
};

use crate::representation::TypeId;
use crate::type_syntax::AnnotationContext;
use crate::written::{WrittenType, parse_written};

/// Where a written name qualifies: a source declaring site (namespace
/// plus `use` tables) or the global context (stub type texts).
pub(crate) enum NameSite<'a> {
    Source {
        namespace: &'a str,
        tables: &'a UseTables,
    },
    /// The global context: stub type texts qualify without namespaces
    /// or `use` tables.
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
/// `callable`). `None` means "an ordinary class name". `pub(crate)`:
/// the type-syntax extension point's `AnnotationSite::keyword_type`
/// shares this table so the native and annotation paths can never
/// disagree.
pub(crate) fn lower_keyword<'db>(db: &'db dyn salsa::Database, name: &str) -> Option<TypeId<'db>> {
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
/// fully qualified name whether or not the class exists. `pub(crate)`:
/// the type-syntax extension point's `AnnotationSite::qualify_class_name`
/// shares this qualifier.
pub(crate) fn qualified_class_name(site: &NameSite<'_>, written: &str) -> String {
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

/// The parsed annotation layer of one member: own docblock, parsed
/// through the type-syntax registry (no registered implementation, or
/// no docblock at all, answers the default — no annotations).
#[derive(Debug, Clone, Default, PartialEq, Eq, salsa::Update)]
pub struct MemberAnnotations<'db> {
    /// `@return` / `@var`: the annotated value type.
    pub value: Option<TypeId<'db>>,
    /// `@param`: annotated parameter types by parameter name.
    pub parameters: Vec<(String, TypeId<'db>)>,
    /// `@throws`: annotated exception types.
    pub throws: Vec<TypeId<'db>>,
}

#[salsa::tracked]
pub fn member_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> MemberAnnotations<'db> {
    // Stub members carry no docblocks (their types come from the
    // signature payload), virtual members have no docblock of their
    // own, and unresolved members have nothing to parse.
    let Some(MemberResolution::Source { member, owner, .. }) =
        lookup_member(db, files, stubs, configuration, query)
    else {
        return MemberAnnotations::default();
    };
    let Some(docblock) = member.docblock.clone() else {
        return MemberAnnotations::default();
    };
    // The declaring site: the owner class-like's namespace and use
    // tables, exactly as native signature resolution derives them —
    // reuse `declaring_site` (via `with_declaring_site`) so the two
    // paths can never disagree.
    let member_key = folded_member_key(member.kind, &member.name);
    let declaring_scope = format!("{owner}::{member_key}");
    let enclosing_docblock = owner_class_docblock(db, files, &owner);
    let parsed = with_declaring_site(db, files, &owner, |site| {
        let context = AnnotationContext {
            declaring_scope: &declaring_scope,
            enclosing_class_scope: Some(&owner),
            enclosing_class_docblock: enclosing_docblock.as_deref(),
        };
        crate::type_syntax::annotations_for_docblock(db, site, &context, &docblock)
    });
    MemberAnnotations {
        value: match member.kind {
            MemberKind::Method => parsed.return_type,
            MemberKind::Property | MemberKind::ClassConstant => parsed.value_type,
            MemberKind::EnumCase => None,
        },
        parameters: parsed.parameters,
        throws: parsed.throws,
    }
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
        let throws_missing = merged.throws.is_empty();
        let missing_parameters: Vec<&String> = parameter_names
            .iter()
            .filter(|name| {
                !merged
                    .parameters
                    .iter()
                    .any(|(merged_name, _)| merged_name == *name)
            })
            .collect();
        if !value_missing && !throws_missing && missing_parameters.is_empty() {
            return merged;
        }
        if !declares(ancestor) {
            continue;
        }
        let ancestor_annotations = read(ancestor);
        if value_missing {
            merged.value = ancestor_annotations.value;
        }
        if throws_missing {
            merged.throws = ancestor_annotations.throws;
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
    let (member, owner) = match lookup_member(db, files, stubs, configuration, query)? {
        MemberResolution::Source { member, owner, .. } => (member, owner),
        MemberResolution::Stub { member, owner } => {
            let range = configuration.php_version_range(db);
            return Some(resolve_stub_member_signature(
                db,
                files,
                stubs,
                configuration,
                range,
                &owner,
                &member,
            ));
        }
        // A virtual member's whole type comes from its annotation text,
        // resolved through the type-syntax registry at the owner's
        // site. There is no native declaration: refinement runs
        // against `mixed`, so any parsed annotation holds
        // (`Trust::Refined`) and an absent or unparseable one stays
        // `(mixed, NativeOnly)`.
        MemberResolution::Virtual { member, owner } => {
            let mixed = TypeId::mixed(db);
            let enclosing_docblock = owner_class_docblock(db, files, &owner);
            return Some(with_declaring_site(db, files, &owner, |site| {
                let context = AnnotationContext {
                    declaring_scope: &owner,
                    enclosing_class_scope: Some(&owner),
                    enclosing_class_docblock: enclosing_docblock.as_deref(),
                };
                let annotation = member.type_text.as_deref().and_then(|text| {
                    crate::type_syntax::type_of_expression(db, site, &context, text)
                });
                let (value_type, value_trust) =
                    refine(db, files, stubs, configuration, mixed, annotation);
                let parameters = member
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let annotation = parameter.type_text.as_deref().and_then(|text| {
                            crate::type_syntax::type_of_expression(db, site, &context, text)
                        });
                        let (parameter_type, trust) =
                            refine(db, files, stubs, configuration, mixed, annotation);
                        DeclaredParameter {
                            name: parameter.name.clone(),
                            parameter_type: Some(parameter_type),
                            trust,
                            optional: parameter.optional,
                            variadic: parameter.variadic,
                            by_reference: false,
                        }
                    })
                    .collect();
                DeclaredSignature {
                    parameters,
                    value_type,
                    value_trust,
                    by_reference: false,
                }
            }));
        }
    };
    let site_parts = declaring_site(db, files, &owner)?;
    let tables = UseTables::for_namespace(item_tree(db, site_parts.file), &site_parts.namespace);
    let site = NameSite::Source {
        namespace: &site_parts.namespace,
        tables: &tables,
    };
    let own = member_annotations(db, files, stubs, configuration, query);
    let root_key = query.class_key(db);
    let class = ClassQuery::new(db, root_key.clone());
    let linearized = linearized_class(db, files, stubs, configuration, class);
    let parameter_names: Vec<String> = member
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
        &owner,
        &member,
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

/// The owner class-like's own docblock text: class-level `@template`
/// declarations are visible inside member annotations.
fn owner_class_docblock(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    owner_key: &str,
) -> Option<String> {
    let site = declaring_site(db, files, owner_key)?;
    member_tree(db, site.file)
        .classes
        .iter()
        .find(|group| group.ast_id == site.ast_id)?
        .docblock
        .clone()
}

/// Borrows one owner's declaring `NameSite` across a closure call and
/// answers the closure's result. The site is `NameSite::Source`
/// (namespace plus `use` tables), built the same way the native
/// signature path builds it; when the owner is not (or no longer) a
/// resolvable source class-like, the closure still runs, against
/// `NameSite::Global` — an unresolvable owner degrades the name
/// qualification, it does not abort the parse.
fn with_declaring_site<T>(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    owner_key: &str,
    parse: impl FnOnce(&NameSite<'_>) -> T,
) -> T {
    match declaring_site(db, files, owner_key) {
        Some(site_parts) => {
            let tables =
                UseTables::for_namespace(item_tree(db, site_parts.file), &site_parts.namespace);
            let site = NameSite::Source {
                namespace: &site_parts.namespace,
                tables: &tables,
            };
            parse(&site)
        }
        None => parse(&NameSite::Global),
    }
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

/// The supported versions inside the configured range, ascending.
fn versions_in_range(range: PhpVersionRange) -> Vec<PhpVersion> {
    SUPPORTED_VERSIONS
        .iter()
        .copied()
        .filter(|version| *version >= range.minimum && *version <= range.maximum)
        .collect()
}

/// One versioned text lowered at one version, in the global context.
/// Missing or malformed text is `mixed` (an undeclared type at that
/// version constrains nothing).
fn lowered_at_version<'db>(
    db: &'db dyn salsa::Database,
    text: &VersionedTypeText,
    version: PhpVersion,
) -> TypeId<'db> {
    text.at(version)
        .and_then(|written| lower_written_text(db, &NameSite::Global, written))
        .unwrap_or_else(|| TypeId::mixed(db))
}

/// The union across the range: the least restrictive reading of a
/// call's result (the parent spec's section 2). A version with no
/// declared text contributes `mixed`. An empty `versions_in_range`
/// (the configured range misses every supported version; unreachable
/// from the composition root today, ranges are clamped to supported
/// versions) answers `mixed`, mirroring the parameter side's
/// degenerate guard: `TypeId::union(db, [])` would otherwise
/// canonicalize to `never`, weaponizing the empty range into a bottom
/// type instead of silencing it.
fn value_type_across_range<'db>(
    db: &'db dyn salsa::Database,
    range: PhpVersionRange,
    text: &VersionedTypeText,
) -> TypeId<'db> {
    let versions = versions_in_range(range);
    if versions.is_empty() {
        return TypeId::mixed(db);
    }
    TypeId::union(
        db,
        versions
            .into_iter()
            .map(|version| lowered_at_version(db, text, version)),
    )
}

/// The most restrictive form across the range, or silence (decision
/// 6): all per-version types equal answers that type; one of them a
/// proven subtype of every other answers that one; otherwise `None` —
/// the empty intersection silences the check instead of weaponizing
/// `never`. Candidates sort by their deterministic rendering before
/// the search, so the winner is independent of override order (two
/// candidates that are each subtypes of all others are equal types,
/// so "first found after the sort" is stable).
fn parameter_type_across_range<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    text: &VersionedTypeText,
) -> Option<TypeId<'db>> {
    let mut distinct: Vec<TypeId<'db>> = versions_in_range(range)
        .into_iter()
        .map(|version| lowered_at_version(db, text, version))
        .collect();
    distinct.sort_by_key(|type_id| type_id.display(db));
    distinct.dedup();
    match distinct.as_slice() {
        [] => Some(TypeId::mixed(db)),
        [single] => Some(*single),
        several => several.iter().copied().find(|candidate| {
            several.iter().all(|other| {
                *other == *candidate
                    || crate::judgments::subtype_of(
                        db,
                        files,
                        stubs,
                        configuration,
                        *candidate,
                        *other,
                    ) == crate::judgments::Proof::Holds
            })
        }),
    }
}

/// One stub member's declared signature under the range rule: the
/// value type is the union across the range, parameters check against
/// their most restrictive form or fall silent, and everything is
/// `Trust::NativeOnly` (stubs carry no annotations to refine with).
fn resolve_stub_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    owner_key: &str,
    member: &StubMember,
) -> DeclaredSignature<'db> {
    let value_only = |value_type: TypeId<'db>| DeclaredSignature {
        parameters: Vec::new(),
        value_type,
        value_trust: Trust::NativeOnly,
        by_reference: false,
    };
    match member.kind {
        StubMemberKind::Method => match member.signature.as_ref() {
            Some(signature) => {
                resolve_stub_signature(db, files, stubs, configuration, range, signature)
            }
            None => value_only(TypeId::mixed(db)),
        },
        StubMemberKind::EnumCase => value_only(TypeId::enum_case(db, owner_key, &member.name)),
        StubMemberKind::ClassConstant => {
            if member.type_text == VersionedTypeText::default() {
                value_only(
                    member
                        .value_text
                        .as_deref()
                        .and_then(|text| literal_type_of_default(db, text))
                        .unwrap_or_else(|| TypeId::mixed(db)),
                )
            } else {
                value_only(value_type_across_range(db, range, &member.type_text))
            }
        }
        StubMemberKind::Property => {
            value_only(value_type_across_range(db, range, &member.type_text))
        }
    }
}

/// One stub callable signature (a function or a method) under the
/// range rule; shared by the member arm and the function query.
fn resolve_stub_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    signature: &StubSignature,
) -> DeclaredSignature<'db> {
    DeclaredSignature {
        parameters: signature
            .parameters
            .iter()
            .map(|parameter| {
                declared_stub_parameter(db, files, stubs, configuration, range, parameter)
            })
            .collect(),
        value_type: value_type_across_range(db, range, &signature.return_type),
        value_trust: Trust::NativeOnly,
        by_reference: signature.by_reference,
    }
}

fn declared_stub_parameter<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    parameter: &StubParameter,
) -> DeclaredParameter<'db> {
    // A parameter that does not span the whole range is optional: a
    // call omitting it must be legal somewhere in the range, so arity
    // never over-reports (a parameter added in 8.2, minimum 8.1).
    let spans_the_whole_range = parameter
        .availability
        .introduced
        .is_none_or(|version| version <= range.minimum)
        && parameter
            .availability
            .removed
            .is_none_or(|version| version > range.maximum);
    DeclaredParameter {
        name: parameter.name.clone(),
        parameter_type: parameter_type_across_range(
            db,
            files,
            stubs,
            configuration,
            range,
            &parameter.type_text,
        ),
        trust: Trust::NativeOnly,
        optional: parameter.optional || !spans_the_whole_range,
        variadic: parameter.variadic,
        by_reference: parameter.by_reference,
    }
}

/// The union of a backed enum's case backing literals, or `None` when
/// the key is not a fully known backed enum: an unresolvable class, no
/// cases at all (a pure enum, or none), or any case with a missing or
/// non-literal backing.
pub(crate) fn enum_backing_union<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    enum_key: &str,
) -> Option<TypeId<'db>> {
    let class = ClassQuery::new(db, enum_key.to_owned());
    let mut literals: Vec<TypeId<'db>> = Vec::new();
    match linearized_class(db, files, stubs, configuration, class).as_ref() {
        Some(linearized) => {
            let cases = linearized
                .members
                .iter()
                .filter(|entry| entry.member.kind == MemberKind::EnumCase);
            for entry in cases {
                let backing = entry.member.signature.default_text.as_deref()?;
                literals.push(literal_type_of_default(db, backing)?);
            }
        }
        None => {
            let table = stub_signature_table(db, stubs);
            let surface = table.class(enum_key)?;
            let cases = surface
                .members
                .iter()
                .filter(|member| member.kind == StubMemberKind::EnumCase);
            for member in cases {
                let backing = member.value_text.as_deref()?;
                literals.push(literal_type_of_default(db, backing)?);
            }
        }
    }
    if literals.is_empty() {
        return None; // a pure enum, or no cases at all
    }
    Some(TypeId::union(db, literals))
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

/// The parsed annotation layer of one free function: own docblock,
/// parsed through the type-syntax registry — the function's exact
/// counterpart to [`member_annotations`]. Functions do not inherit
/// (there is no ancestor to walk): a stub-only function, an absent
/// source declaration, or a source declaration with no docblock all
/// answer the default. `stubs` and `configuration` complete the
/// input quartet the annotation seam shares with `member_annotations`
/// (a uniform query shape callers never have to special-case); a
/// free function's own docblock resolves without consulting either.
#[salsa::tracked]
pub fn function_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> MemberAnnotations<'db> {
    let symbol_query = SymbolQuery::new(db, SymbolSpace::Function, query.key(db).clone());
    let Some(ast_id) = lookup_function_declaration(db, files, symbol_query) else {
        return MemberAnnotations::default();
    };
    let index = analyzed_file_index(db, files);
    let Ok(position) = index.binary_search_by_key(&ast_id.file, |(id, _)| *id) else {
        return MemberAnnotations::default();
    };
    let Some(&(_, file)) = index.get(position) else {
        return MemberAnnotations::default();
    };
    let Some(function) = member_tree(db, file)
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)
        .cloned()
    else {
        return MemberAnnotations::default();
    };
    let Some(docblock) = function.docblock.clone() else {
        return MemberAnnotations::default();
    };
    let tables = UseTables::for_namespace(item_tree(db, file), &function.namespace);
    let site = NameSite::Source {
        namespace: &function.namespace,
        tables: &tables,
    };
    let function_key = query.key(db).clone();
    let context = AnnotationContext {
        declaring_scope: &function_key,
        enclosing_class_scope: None,
        enclosing_class_docblock: None,
    };
    let parsed = crate::type_syntax::annotations_for_docblock(db, &site, &context, &docblock);
    MemberAnnotations {
        value: parsed.return_type,
        parameters: parsed.parameters,
        throws: parsed.throws,
    }
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
    let Some(ast_id) = lookup_function_declaration(db, files, symbol_query) else {
        // No source declaration: consult the stub signature table
        // under the range rule. Layering rule: existence is the
        // symbol table's answer (it is availability-filtered against
        // the configured range); the signature table is version-
        // agnostic by design and must never be asked "does this exist
        // here" on its own — a stub function absent from the whole
        // configured range must answer `None`, not a signature.
        let range = configuration.php_version_range(db);
        let in_range = celerrate_semantics::stub_symbol_table(db, stubs, configuration)
            .lookup(SymbolSpace::Function, query.key(db))
            .is_some();
        if !in_range {
            return None;
        }
        return stub_signature_table(db, stubs)
            .function(query.key(db))
            .map(|signature| {
                resolve_stub_signature(db, files, stubs, configuration, range, signature)
            });
    };
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
    let annotations = function_annotations(db, files, stubs, configuration, query);
    let native_value = lowered_or_mixed(db, &site, function.signature.type_text.as_deref());
    let (value_type, value_trust) = refine(
        db,
        files,
        stubs,
        configuration,
        native_value,
        annotations.value,
    );
    Some(DeclaredSignature {
        parameters: function
            .signature
            .parameters
            .iter()
            .map(|parameter| {
                let annotation = annotations
                    .parameters
                    .iter()
                    .find(|(name, _)| *name == parameter.name)
                    .map(|(_, annotated)| *annotated);
                declared_parameter(
                    db,
                    files,
                    stubs,
                    configuration,
                    &site,
                    parameter,
                    annotation,
                )
            })
            .collect(),
        value_type,
        value_trust,
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
        // A leading-backslash name is never a keyword either, even
        // when the trimmed name matches one exactly.
        assert_eq!(lower(&db, "\\int"), Some(TypeId::class(&db, "int", vec![])),);
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
        MemberKind, MemberQuery, PluginIdentity, SymbolSpace, VirtualMember, VirtualMemberKind,
        VirtualParameter, VirtualSymbolProvider, VirtualSymbolRegistration, VirtualSymbolRegistry,
        folded_member_key, folded_symbol_key,
    };
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubClassSurface, StubIndex, StubIndexInput, StubMember, StubMemberKind,
        StubParameter, StubSignature, StubSymbol, StubSymbolKind, StubVisibility,
        VersionedTypeText,
    };

    use super::{
        DeclaredSignature, FunctionQuery, Trust, declared_function_signature,
        declared_member_signature,
    };
    use crate::type_syntax::{
        AnnotationSite, ParsedAnnotations, TypeSyntax, TypeSyntaxRegistration, TypeSyntaxRegistry,
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

    /// A folded `MemberQuery` for one class-and-member pair, the shape
    /// every member-facing test needs to build.
    fn member_query<'db>(
        fixture: &'db Fixture,
        class_written: &str,
        kind: MemberKind,
        member_written: &str,
    ) -> MemberQuery<'db> {
        MemberQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, class_written),
            kind,
            folded_member_key(kind, member_written),
        )
    }

    /// Registers a `TypeSyntax` fake that parses any docblock
    /// containing `@return` to `return_type: Some(int)`; everything
    /// else in `ParsedAnnotations` stays default. Its bare-expression
    /// path answers "int" -> int and refuses anything else, covering
    /// both the parsed and unparseable virtual-member cases. Duplicated
    /// from `type_syntax`'s test module `FakeSyntax` (recorded debt: no
    /// shared test-support module per the design).
    fn register_fake_syntax(fixture: &Fixture) {
        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-return"),
            implementation: std::sync::Arc::new(FakeReturnSyntax),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    /// Registers a `TypeSyntax` fake that parses any docblock
    /// containing `@tags` to BOTH `return_type: Some(int)` and
    /// `value_type: Some(string)` — proving the kind-based pick in
    /// `member_annotations`: methods read `return_type`, properties
    /// and class constants read `value_type`.
    fn register_fake_syntax_both(fixture: &Fixture) {
        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-both"),
            implementation: std::sync::Arc::new(FakeBothSyntax),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    fn fake_identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    /// A provider that answers its fixed member set only when the
    /// class docblock carries `@fake`. Duplicated from
    /// `celerrate_semantics`'s own test modules (`linearize.rs`,
    /// `member_lookup.rs`, `virtual_symbols.rs`) — recorded debt, no
    /// shared test-support module exists across crates.
    #[derive(Debug)]
    struct FakeProvider {
        members: Vec<VirtualMember>,
    }

    impl VirtualSymbolProvider for FakeProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            if class_docblock.contains("@fake") {
                self.members.clone()
            } else {
                Vec::new()
            }
        }
    }

    fn register_fake_virtual_provider(fixture: &Fixture, members: Vec<VirtualMember>) {
        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: fake_identity("fake-virtual"),
            provider: std::sync::Arc::new(FakeProvider { members }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    /// A non-static virtual property with an annotated type text and
    /// no parameters.
    fn virtual_property_with_text(name: &str, type_text: &str) -> VirtualMember {
        VirtualMember {
            kind: VirtualMemberKind::Property,
            name: name.to_owned(),
            is_static: false,
            type_text: Some(type_text.to_owned()),
            parameters: Vec::new(),
        }
    }

    #[derive(Debug)]
    struct FakeReturnSyntax;

    impl TypeSyntax for FakeReturnSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains("@return")
        }
        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            _docblock: &str,
        ) -> ParsedAnnotations<'db> {
            ParsedAnnotations {
                return_type: Some(TypeId::int(site.database())),
                ..ParsedAnnotations::default()
            }
        }
        fn parse_type_expression<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            expression: &str,
        ) -> Option<TypeId<'db>> {
            // Answers "int" -> int and refuses anything else, so tests
            // exercising virtual-member typing can prove both the
            // parsed and the unparseable path through this one fake.
            (expression == "int").then(|| TypeId::int(site.database()))
        }
    }

    /// A `TypeSyntax` fake that parses `@return-class <Name>` into a
    /// resolved class type through `site.qualify_class_name`, proving
    /// that the function annotation seam resolves class names at the
    /// function's own declaring site.
    #[derive(Debug)]
    struct FakeClassReturnSyntax;

    impl TypeSyntax for FakeClassReturnSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains("@return-class")
        }
        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            docblock: &str,
        ) -> ParsedAnnotations<'db> {
            let name = docblock
                .split("@return-class")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or_default();
            let qualified = site.qualify_class_name(name);
            ParsedAnnotations {
                return_type: Some(TypeId::class(site.database(), &qualified, vec![])),
                ..ParsedAnnotations::default()
            }
        }
        fn parse_type_expression<'db>(
            &self,
            _site: &AnnotationSite<'db, '_>,
            _expression: &str,
        ) -> Option<TypeId<'db>> {
            None
        }
    }

    fn register_fake_class_syntax(fixture: &Fixture) {
        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-return-class"),
            implementation: std::sync::Arc::new(FakeClassReturnSyntax),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    #[derive(Debug)]
    struct FakeBothSyntax;

    impl TypeSyntax for FakeBothSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains("@tags")
        }
        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            _docblock: &str,
        ) -> ParsedAnnotations<'db> {
            let db = site.database();
            ParsedAnnotations {
                return_type: Some(TypeId::int(db)),
                value_type: Some(TypeId::string(db)),
                ..ParsedAnnotations::default()
            }
        }
        fn parse_type_expression<'db>(
            &self,
            _site: &AnnotationSite<'db, '_>,
            _expression: &str,
        ) -> Option<TypeId<'db>> {
            None
        }
    }

    fn member<'db>(
        fixture: &'db Fixture,
        class_written: &str,
        kind: MemberKind,
        member_written: &str,
    ) -> Option<DeclaredSignature<'db>> {
        let query = member_query(fixture, class_written, kind, member_written);
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
    fn a_function_docblock_parses_through_the_registry() {
        let fixture = fixture(&["<?php /** @return int */ function f(): string {}"]);
        register_fake_syntax(&fixture);
        let query = FunctionQuery::new(&fixture.db, folded_symbol_key(SymbolSpace::Function, "f"));
        let annotations = super::function_annotations(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        );
        assert_eq!(annotations.value, Some(TypeId::int(&fixture.db)));
    }

    #[test]
    fn the_function_signature_refines_under_the_trust_rule() {
        // int <: string fails: the annotation is rejected, native wins.
        let fixture = fixture(&["<?php /** @return int */ function f(): string {}"]);
        register_fake_syntax(&fixture);
        let query = FunctionQuery::new(&fixture.db, folded_symbol_key(SymbolSpace::Function, "f"));
        let signature = declared_function_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(signature.value_type, TypeId::string(&fixture.db));
        assert_eq!(signature.value_trust, Trust::RejectedAnnotation);
    }

    #[test]
    fn an_unannotated_function_stays_native_only() {
        let fixture = fixture(&["<?php function f(): string {}"]);
        register_fake_syntax(&fixture);
        let query = FunctionQuery::new(&fixture.db, folded_symbol_key(SymbolSpace::Function, "f"));
        let signature = declared_function_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn a_function_signature_refines_through_a_resolved_class_annotation() {
        // `@return-class Dog` against a native `Animal` return: Dog is
        // a proven subtype, so the annotation refines (Holds).
        let fixture = fixture(
            &["<?php interface Animal {} class Dog implements Animal {}\n\
             /** @return-class Dog */ function f(): Animal {}"],
        );
        register_fake_class_syntax(&fixture);
        let query = FunctionQuery::new(&fixture.db, folded_symbol_key(SymbolSpace::Function, "f"));
        let signature = declared_function_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        let db = &fixture.db;
        assert_eq!(signature.value_type, TypeId::class(db, "Dog", vec![]));
        assert_eq!(signature.value_trust, Trust::Refined);
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
    fn the_annotation_seam_answers_the_default_with_no_registered_syntax() {
        // No registry, no annotations — the no-plugin path every test
        // database takes.
        let fixture = fixture(&["<?php class C { /** @return int */ public function f() {} }"]);
        let query = member_query(&fixture, "C", MemberKind::Method, "f");
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
    fn the_seam_parses_the_own_docblock_through_the_registry() {
        let fixture =
            fixture(&["<?php class C { /** @return int */ public function f(): string {} }"]);
        register_fake_syntax(&fixture);
        let query = member_query(&fixture, "C", MemberKind::Method, "f");
        let annotations = super::member_annotations(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        );
        assert_eq!(annotations.value, Some(TypeId::int(&fixture.db)));
    }

    #[test]
    fn the_value_annotation_is_picked_by_member_kind() {
        // A fake syntax answering BOTH return_type=int and
        // value_type=string proves the kind-based pick: methods read
        // @return, properties @var.
        let fixture = fixture(&[
            "<?php class C { /** @tags */ public $p; /** @tags */ public function f() {} }",
        ]);
        register_fake_syntax_both(&fixture);
        let property = member_query(&fixture, "C", MemberKind::Property, "p");
        let method = member_query(&fixture, "C", MemberKind::Method, "f");
        let db = &fixture.db;
        assert_eq!(
            super::member_annotations(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                property
            )
            .value,
            Some(TypeId::string(db)),
        );
        assert_eq!(
            super::member_annotations(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                method
            )
            .value,
            Some(TypeId::int(db)),
        );
    }

    #[test]
    fn stub_and_missing_members_answer_the_default() {
        let fixture = fixture(&["<?php class C {}"]);
        register_fake_syntax(&fixture);
        let query = member_query(&fixture, "C", MemberKind::Method, "ghost");
        assert_eq!(
            super::member_annotations(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                query,
            ),
            super::MemberAnnotations::default(),
        );
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
                    throws: Vec::new(),
                },
                // The far ancestor annotates both.
                "far" => super::MemberAnnotations {
                    value: Some(int),
                    parameters: vec![("x".to_owned(), bool_type)],
                    throws: Vec::new(),
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
            throws: Vec::new(),
        };
        let merged = super::inherited_annotations(
            own.clone(),
            &[],
            &["ancestor".to_owned()],
            |_| true,
            |_| super::MemberAnnotations {
                value: Some(string),
                parameters: vec![],
                throws: Vec::new(),
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
                throws: Vec::new(),
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

    /// A fixture whose stub payload carries function signatures and
    /// class surfaces. Every named function, class, and parent becomes
    /// a `StubSymbol`, so lookups resolve.
    fn fixture_with_stub_payload(
        sources: &[&str],
        functions: Vec<(String, StubSignature)>,
        classes: Vec<(String, StubClassSurface)>,
    ) -> Fixture {
        fixture_with_stub_payload_in_range(
            sources,
            functions,
            classes,
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        )
    }

    fn fixture_with_stub_payload_in_range(
        sources: &[&str],
        functions: Vec<(String, StubSignature)>,
        classes: Vec<(String, StubClassSurface)>,
        range: PhpVersionRange,
    ) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let mut symbols: Vec<StubSymbol> = functions
            .iter()
            .map(|(name, _)| StubSymbol {
                name: name.clone(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            })
            .collect();
        let mut class_names: Vec<String> = Vec::new();
        for (name, surface) in &classes {
            class_names.push(name.clone());
            for parent in &surface.parents {
                class_names.push(parent.clone());
            }
        }
        class_names.sort();
        class_names.dedup();
        symbols.extend(class_names.into_iter().map(|name| StubSymbol {
            name,
            kind: StubSymbolKind::Class,
            availability: StubAvailability::ALWAYS,
        }));
        let stubs = StubIndexInput::builder(StubIndex::new(symbols, functions, classes))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn function<'db>(fixture: &'db Fixture, written: &str) -> Option<DeclaredSignature<'db>> {
        let query = FunctionQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::Function, written),
        );
        declared_function_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    fn stub_parameter(name: &str, type_text: Option<&str>) -> StubParameter {
        StubParameter {
            name: name.to_owned(),
            type_text: VersionedTypeText::from_text(type_text.map(str::to_owned)),
            optional: false,
            by_reference: false,
            variadic: false,
            availability: StubAvailability::ALWAYS,
        }
    }

    /// A `getMessage(): string` public instance method surface member.
    fn get_message_member() -> StubMember {
        StubMember {
            kind: StubMemberKind::Method,
            name: "getMessage".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: Some(StubSignature {
                parameters: vec![],
                return_type: VersionedTypeText::from_text(Some("string".to_owned())),
                by_reference: false,
            }),
            type_text: VersionedTypeText::default(),
            value_text: None,
        }
    }

    /// A one-parameter signature whose forms across the range have no
    /// most-restrictive member: `int` at 8.1, `string` from 8.2.
    fn disjoint_signature() -> StubSignature {
        StubSignature {
            parameters: vec![StubParameter {
                name: "value".to_owned(),
                type_text: VersionedTypeText {
                    default: Some("int".to_owned()),
                    overrides: vec![(PhpVersion::new(8, 2), "string".to_owned())],
                },
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText::from_text(Some("void".to_owned())),
            by_reference: false,
        }
    }

    #[test]
    fn a_stub_function_resolves_with_union_returns_across_the_range() {
        let strlen = StubSignature {
            parameters: vec![
                stub_parameter("string", Some("string")),
                // Untyped at every version: `mixed`, never silenced.
                stub_parameter("anything", None),
            ],
            return_type: VersionedTypeText {
                default: Some("int".to_owned()),
                overrides: vec![(PhpVersion::new(8, 3), "int|false".to_owned())],
            },
            by_reference: false,
        };
        let fixture =
            fixture_with_stub_payload(&["<?php"], vec![("strlen".to_owned(), strlen)], vec![]);
        let db = &fixture.db;
        let signature = function(&fixture, "strlen").unwrap();
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::string(db))
        );
        // An untyped stub parameter is `Some(mixed)`, never `None`.
        assert_eq!(
            signature.parameters[1].parameter_type,
            Some(TypeId::mixed(db))
        );
        // Union across 8.1..8.5: int at 8.1-8.2, int|false at 8.3+.
        assert_eq!(
            signature.value_type,
            TypeId::union(db, [TypeId::int(db), TypeId::bool_literal(db, false)]),
        );
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn a_version_gap_in_the_return_text_widens_the_union_to_mixed() {
        // No default text and an override only from 8.3: 8.1 and 8.2
        // have no declared text, so `lowered_at_version` answers
        // `mixed` for them, and the union absorbs to `mixed`.
        let gapped = StubSignature {
            parameters: vec![],
            return_type: VersionedTypeText {
                default: None,
                overrides: vec![(PhpVersion::new(8, 3), "int".to_owned())],
            },
            by_reference: false,
        };
        let fixture =
            fixture_with_stub_payload(&["<?php"], vec![("gapped".to_owned(), gapped)], vec![]);
        let db = &fixture.db;
        let signature = function(&fixture, "gapped").unwrap();
        assert_eq!(signature.value_type, TypeId::mixed(db));
    }

    #[test]
    fn a_parameter_narrowing_across_the_range_takes_the_most_restrictive_form() {
        // `int` at 8.1 versus `int|string` at 8.2+: `int` is a proven
        // subtype of every per-version form, so `int` is the check type.
        let signature = StubSignature {
            parameters: vec![StubParameter {
                name: "value".to_owned(),
                type_text: VersionedTypeText {
                    default: Some("int".to_owned()),
                    overrides: vec![(PhpVersion::new(8, 2), "int|string".to_owned())],
                },
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText::from_text(Some("void".to_owned())),
            by_reference: false,
        };
        let fixture = fixture_with_stub_payload(
            &["<?php"],
            vec![("narrowing".to_owned(), signature)],
            vec![],
        );
        let db = &fixture.db;
        let declared = function(&fixture, "narrowing").unwrap();
        assert_eq!(declared.parameters[0].parameter_type, Some(TypeId::int(db)));
    }

    #[test]
    fn a_disjoint_parameter_across_the_range_is_silenced() {
        // `int` at 8.1, `string` from 8.2: no most-restrictive form
        // exists — the design's degenerate guard silences the parameter.
        let fixture = fixture_with_stub_payload(
            &["<?php"],
            vec![("disjoint".to_owned(), disjoint_signature())],
            vec![],
        );
        let declared = function(&fixture, "disjoint").unwrap();
        assert_eq!(declared.parameters[0].parameter_type, None);
        assert!(!declared.parameters[0].optional, "silencing is type-only");
    }

    #[test]
    fn a_parameter_added_inside_the_range_is_optional() {
        let signature = StubSignature {
            parameters: vec![StubParameter {
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 3)),
                    removed: None,
                    deprecated: None,
                },
                ..stub_parameter("added", Some("string"))
            }],
            return_type: VersionedTypeText::from_text(Some("void".to_owned())),
            by_reference: false,
        };
        let fixture =
            fixture_with_stub_payload(&["<?php"], vec![("grown".to_owned(), signature)], vec![]);
        let declared = function(&fixture, "grown").unwrap();
        // The parameter does not span the whole range: a call omitting
        // it must be legal at 8.1-8.2, so arity never over-reports.
        assert!(declared.parameters[0].optional);
    }

    #[test]
    fn stub_member_signatures_resolve_through_the_same_rule() {
        let fixture = fixture_with_stub_payload(
            &["<?php class MyError extends Exception {}"],
            vec![],
            vec![(
                "Exception".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![get_message_member()],
                },
            )],
        );
        let db = &fixture.db;
        let signature = member(&fixture, "MyError", MemberKind::Method, "getMessage").unwrap();
        assert_eq!(signature.value_type, TypeId::string(db));
        assert_eq!(signature.value_trust, Trust::NativeOnly);
        // Stub types resolve in the global context, straight off the
        // stub class too (no source class in between).
        let direct = member(&fixture, "Exception", MemberKind::Method, "getmessage").unwrap();
        assert_eq!(direct.value_type, TypeId::string(db));
    }

    /// A `$code` property surface member whose type narrows across the
    /// range: `int` at 8.1-8.2, `string` from 8.3.
    fn code_property_member() -> StubMember {
        StubMember {
            kind: StubMemberKind::Property,
            name: "code".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: None,
            type_text: VersionedTypeText {
                default: Some("int".to_owned()),
                overrides: vec![(PhpVersion::new(8, 3), "string".to_owned())],
            },
            value_text: None,
        }
    }

    /// A `NAME` class constant surface member with no written type: an
    /// untyped constant with a literal default carries the literal.
    fn name_constant_member() -> StubMember {
        StubMember {
            kind: StubMemberKind::ClassConstant,
            name: "NAME".to_owned(),
            visibility: StubVisibility::Public,
            is_static: true,
            availability: StubAvailability::ALWAYS,
            signature: None,
            type_text: VersionedTypeText::default(),
            value_text: Some("'active'".to_owned()),
        }
    }

    /// An `Active` enum-case surface member: its type is its identity.
    fn active_case_member() -> StubMember {
        StubMember {
            kind: StubMemberKind::EnumCase,
            name: "Active".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: None,
            type_text: VersionedTypeText::default(),
            value_text: None,
        }
    }

    #[test]
    fn a_stub_property_resolves_its_type_across_the_range() {
        let fixture = fixture_with_stub_payload(
            &["<?php"],
            vec![],
            vec![(
                "Status".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![code_property_member()],
                },
            )],
        );
        let db = &fixture.db;
        let signature = member(&fixture, "Status", MemberKind::Property, "code").unwrap();
        assert_eq!(
            signature.value_type,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
        );
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn a_stub_class_constant_with_no_type_text_carries_its_literal_value() {
        let fixture = fixture_with_stub_payload(
            &["<?php"],
            vec![],
            vec![(
                "Status".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![name_constant_member()],
                },
            )],
        );
        let db = &fixture.db;
        let signature = member(&fixture, "Status", MemberKind::ClassConstant, "NAME").unwrap();
        assert_eq!(signature.value_type, TypeId::string_literal(db, "active"));
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn a_stub_enum_case_answers_its_own_identity() {
        let fixture = fixture_with_stub_payload(
            &["<?php"],
            vec![],
            vec![(
                "Status".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![active_case_member()],
                },
            )],
        );
        let db = &fixture.db;
        let signature = member(&fixture, "Status", MemberKind::EnumCase, "Active").unwrap();
        assert_eq!(
            signature.value_type,
            TypeId::enum_case(db, "Status", "Active"),
        );
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }

    #[test]
    fn a_point_range_never_silences() {
        // With min == max the "range" is one version: every parameter
        // has exactly one form, so the degenerate guard never fires.
        let fixture = fixture_with_stub_payload_in_range(
            &["<?php"],
            vec![("disjoint".to_owned(), disjoint_signature())],
            vec![],
            PhpVersionRange::point(PhpVersion::new(8, 4)),
        );
        let db = &fixture.db;
        let declared = function(&fixture, "disjoint").unwrap();
        assert_eq!(
            declared.parameters[0].parameter_type,
            Some(TypeId::string(db))
        );
    }

    #[test]
    fn an_empty_supported_version_window_widens_the_stub_return_type_to_mixed() {
        // The configured range (a point at 9.0) misses every supported
        // version (8.1-8.5): `versions_in_range` is empty. The parameter
        // side already has a degenerate guard for this (empty -> mixed);
        // the value side must match it rather than folding an empty
        // union to `never` (the exact "weaponized never" the guard
        // exists to prevent). Unreachable from the composition root
        // today (ranges are clamped to supported versions during
        // discovery), so this fixture constructs it directly through
        // the stub-signature path.
        let strlen = StubSignature {
            parameters: vec![],
            return_type: VersionedTypeText::from_text(Some("int".to_owned())),
            by_reference: false,
        };
        let fixture = fixture_with_stub_payload_in_range(
            &["<?php"],
            vec![("strlen".to_owned(), strlen)],
            vec![],
            PhpVersionRange::point(PhpVersion::new(9, 0)),
        );
        let db = &fixture.db;
        let signature = function(&fixture, "strlen").unwrap();
        assert_eq!(signature.value_type, TypeId::mixed(db));
    }

    #[test]
    fn a_stub_function_outside_the_configured_range_answers_no_signature() {
        // The signature table carries a payload for `futureOnly`, but
        // its `StubSymbol` availability starts at 8.6 — entirely
        // outside the fixture's 8.1-8.5 configured range. Existence is
        // the availability-filtered symbol table's answer, not the
        // (version-agnostic) signature table's: the fallback must
        // answer `None`, never a signature for a function that does
        // not exist anywhere in range.
        let db = TestDatabase::default();
        let files = AnalyzedFileSet::new(
            &db,
            vec![SourceFile::new(&db, FileId::new(0), b"<?php".to_vec())],
        );
        let signature = StubSignature {
            parameters: vec![],
            return_type: VersionedTypeText::from_text(Some("string".to_owned())),
            by_reference: false,
        };
        let symbols = vec![StubSymbol {
            name: "futureOnly".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability {
                introduced: Some(PhpVersion::new(8, 6)),
                removed: None,
                deprecated: None,
            },
        }];
        let stubs = StubIndexInput::builder(StubIndex::new(
            symbols,
            vec![("futureOnly".to_owned(), signature)],
            vec![],
        ))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        let fixture = Fixture {
            db,
            files,
            stubs,
            configuration,
        };
        assert!(function(&fixture, "futureOnly").is_none());
    }

    #[test]
    fn a_virtual_property_types_through_the_type_syntax_registry() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_virtual_provider(&fixture, vec![virtual_property_with_text("title", "int")]);
        register_fake_syntax(&fixture); // parse_type_expression: "int" -> int
        let query = member_query(&fixture, "Post", MemberKind::Property, "title");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(signature.value_type, TypeId::int(&fixture.db));
        assert_eq!(signature.value_trust, Trust::Refined);
    }

    #[test]
    fn a_virtual_method_carries_its_annotated_parameters() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_virtual_provider(
            &fixture,
            vec![VirtualMember {
                kind: VirtualMemberKind::Method,
                name: "find".to_owned(),
                is_static: true,
                type_text: Some("int".to_owned()),
                parameters: vec![VirtualParameter {
                    name: "id".to_owned(),
                    type_text: Some("int".to_owned()),
                    optional: false,
                    variadic: false,
                }],
            }],
        );
        register_fake_syntax(&fixture);
        let query = member_query(&fixture, "Post", MemberKind::Method, "find");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(signature.value_type, TypeId::int(&fixture.db));
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].name, "id");
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::int(&fixture.db))
        );
    }

    #[test]
    fn an_unparseable_virtual_type_degrades_to_mixed_native_only() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_virtual_provider(
            &fixture,
            vec![virtual_property_with_text("title", "no<such>notation")],
        );
        register_fake_syntax(&fixture);
        let query = member_query(&fixture, "Post", MemberKind::Property, "title");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(signature.value_type, TypeId::mixed(&fixture.db));
        assert_eq!(signature.value_trust, Trust::NativeOnly);
    }
}
