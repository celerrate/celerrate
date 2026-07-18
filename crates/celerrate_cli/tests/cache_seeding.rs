//! The cache snapshot seeds a fresh session. These tests hand-write
//! pack files and observe the session serving from them; the probe
//! entries deliberately violate the exactness contract, because a
//! correct entry is indistinguishable from a recomputation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::analysis::analyze;
use celerrate_cli::cache::pack::{Pack, PackHeader, encode, write_atomically};
use celerrate_cli::cache::snapshot::{
    DIAGNOSTICS_PACK, INFERRED_SIGNATURES_PACK, ITEM_TREES_PACK, MEMBER_TREES_PACK,
};
use celerrate_cli::cache::stored::{
    StoredAnswer, StoredDiagnostic, StoredItemTree, StoredMemberTree, StoredRecord, StoredSeverity,
    StoredSpace, StoredVerdict,
};
use celerrate_cli::session::Session;
use celerrate_project::{PhpVersion, PhpVersionRange};
use celerrate_semantics::{
    AstId, ClassMembers, Declaration, DeclarationKind, ItemTree, MemberTree, SymbolSpace,
    item_tree, member_tree, source_symbol_table,
};
use celerrate_types::{StoredInferredSignature, StoredSignatureKey};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn write_item_trees_pack(
    root: &Path,
    header: &PackHeader,
    entries: Vec<([u8; 32], StoredItemTree)>,
) {
    let directory = root.join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = encode(&Pack {
        header: header.clone(),
        entries,
    })
    .unwrap();
    write_atomically(&directory.join(ITEM_TREES_PACK), &bytes).unwrap();
}

fn write_member_trees_pack(
    root: &Path,
    header: &PackHeader,
    entries: Vec<([u8; 32], StoredMemberTree)>,
) {
    let directory = root.join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = encode(&Pack {
        header: header.clone(),
        entries,
    })
    .unwrap();
    write_atomically(&directory.join(MEMBER_TREES_PACK), &bytes).unwrap();
}

/// The probe: an empty stored tree for a file that declares one class.
/// A session that serves it consulted the pack; a session that lowers
/// the file would see the declaration.
#[test]
fn a_matching_pack_seeds_the_item_tree_query() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    // The content hash must be computed exactly as the session will:
    // over the file's raw bytes.
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_item_trees_pack(root.path(), &header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    let tree = item_tree(&session.database, file);
    assert!(
        tree.declarations.is_empty(),
        "the probe tree is served from the pack, not lowered from source",
    );
}

/// The member-tree sibling of the item-tree probe above: the pack's
/// entry deliberately differs from the true projection (an empty
/// member tree for a file that declares a member-bearing class), so a
/// session serving it proves the `member_tree` query consulted the
/// pack rather than lowering the file.
#[test]
fn a_matching_pack_seeds_the_member_tree_query() {
    let source = "<?php class Marker { public function m() {} }";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredMemberTree::of(&MemberTree::default());
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_member_trees_pack(root.path(), &header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    let tree = member_tree(&session.database, file);
    assert!(
        tree.classes.is_empty(),
        "the probe tree is served from the pack, not lowered from source",
    );
}

/// The probe's `defines` field, not the source, must reach the symbol
/// table: the file below declares no `define()` at all, so a resolved
/// lookup under the probe's fabricated name proves the pack's `defines`
/// list is what the table was built from, not a reparse of the source.
/// This is the persistent half of the fix: `source_symbol_table` used to
/// answer `defined_constants`, a per-file query with no `ArtifactCache`
/// hook, so a fresh process paid a full reparse for it; now it reads
/// `item_tree(..).defines`, which the pack already covers.
#[test]
fn a_matching_pack_seeds_the_symbol_table_with_its_defines() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut probe_tree = ItemTree::default();
    probe_tree.defines.push("PROBE_ROOT".to_owned());
    let probe = StoredItemTree::of(&probe_tree);
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_item_trees_pack(root.path(), &header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let table = source_symbol_table(&session.database, session.files);
    assert!(
        table.lookup(SymbolSpace::Constant, "PROBE_ROOT").is_some(),
        "the pack's define must reach the table, since the source declares none",
    );
}

/// A pack written under another PHP version range is ignored whole.
#[test]
fn a_range_mismatch_ignores_the_pack() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let other_header = PackHeader::current(
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 2)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_item_trees_pack(root.path(), &other_header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(
        item_tree(&session.database, file).declarations.len(),
        1,
        "the mismatched pack is ignored and the file is lowered",
    );
}

/// A pack written under another stub snapshot is ignored whole (audit
/// finding I3): the field's whole purpose is "a new snapshot changes
/// availability answers", and it was the one header field no test
/// proved discards the pack.
#[test]
fn a_stub_blob_mismatch_ignores_the_pack() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let mut foreign_stub = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    foreign_stub.stub_blob[0] ^= 0xFF;
    write_item_trees_pack(root.path(), &foreign_stub, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(
        item_tree(&session.database, file).declarations.len(),
        1,
        "the mismatched pack is ignored and the file is lowered",
    );
}

/// A corrupt pack is silently absent.
#[test]
fn a_corrupt_pack_is_silently_absent() {
    let root = project(&[("a.php", "<?php class Marker {}")]);
    let directory = root.path().join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(ITEM_TREES_PACK), b"garbage").unwrap();

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(item_tree(&session.database, file).declarations.len(), 1);
    assert!(
        session.internal_errors.is_empty(),
        "corruption is never an error the user sees",
    );
}

fn write_diagnostics_pack(
    root: &Path,
    header: &PackHeader,
    entries: Vec<([u8; 32], StoredVerdict)>,
) {
    let directory = root.join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = encode(&Pack {
        header: header.clone(),
        entries,
    })
    .unwrap();
    write_atomically(&directory.join(DIAGNOSTICS_PACK), &bytes).unwrap();
}

fn probe_verdict() -> StoredVerdict {
    StoredVerdict {
        diagnostics: vec![StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 10,
            end: 17,
            message: "planted by the cache probe".to_owned(),
        }],
        records: vec![StoredRecord {
            written: "Missing".to_owned(),
            space: StoredSpace::ClassLike,
            namespace: String::new(),
            answer: StoredAnswer::Unknown,
        }],
    }
}

/// The source references `Missing`, which resolves to nothing: the
/// recorded `Unknown` answer still holds, so the planted verdict is
/// served instead of a recomputation.
#[test]
fn a_verdict_whose_records_still_hold_is_served() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, probe_verdict())]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    let messages: Vec<&str> = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec!["planted by the cache probe"],
        "the stored verdict speaks, not a recomputation",
    );
}

/// A defining file appeared since the verdict was recorded: `Missing`
/// now resolves to a source declaration, the recorded `Unknown` answer
/// no longer holds, and the entry is discarded — the planted probe
/// must not survive.
#[test]
fn a_verdict_whose_answer_flipped_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source), ("b.php", "<?php class Missing {}")]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, probe_verdict())]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "planted by the cache probe"),
        "a stale verdict must be recomputed: {:?}",
        outcome.diagnostics,
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "Missing now resolves, so the honest answer is no diagnostics",
    );
}

/// An entry carrying an identifier this binary does not know is from
/// another era: the whole entry is discarded and the file recomputed.
#[test]
fn a_verdict_with_an_unknown_identifier_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].id = "CEL9999".to_owned();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1, "recomputed honestly");
    assert!(outcome.diagnostics[0].message.contains("Missing"));
}

/// A crafted entry whose stored range has `start > end` cannot come from
/// any real computation: `TextRange::new` asserts `start <= end` and
/// panics if it does not. The blake3 checksum a hostile pack carries only
/// proves nothing bit-flipped in transit, not that whoever wrote the pack
/// was honest, so a reversed range must be caught before it ever reaches
/// `TextRange::new`. The entry is discarded and the file recomputed, with
/// no panic and no internal error surfaced to the user.
#[test]
fn a_verdict_with_a_reversed_range_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].start = 17;
    verdict.diagnostics[0].end = 10;
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert!(
        outcome.panicked.is_empty(),
        "a crafted reversed range must never panic the analysis: {:?}",
        outcome.panicked,
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "planted by the cache probe"),
        "a malformed entry must not be served: {:?}",
        outcome.diagnostics,
    );
    assert_eq!(outcome.diagnostics.len(), 1, "recomputed honestly");
    assert!(outcome.diagnostics[0].message.contains("Missing"));
}

use celerrate_cli::run;

fn run_check(root: &Path) -> (celerrate_cli::Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_completed_run_writes_every_pack_and_the_gitignore() {
    let root = project(&[("a.php", "<?php class A {} new Missing();")]);
    let (_, _) = run_check(root.path());

    let cache = root.path().join(".celerrate/cache");
    assert!(cache.join(ITEM_TREES_PACK).is_file());
    assert!(cache.join(MEMBER_TREES_PACK).is_file());
    assert!(cache.join(DIAGNOSTICS_PACK).is_file());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".celerrate/.gitignore")).unwrap(),
        "*\n",
        "the cache directory ignores itself, like Cargo's target directory",
    );
}

#[test]
fn the_written_packs_validate_and_carry_the_analyzed_files() {
    let source_a = "<?php class A {}";
    let source_b = "<?php new Missing();";
    let root = project(&[("a.php", source_a), ("b.php", source_b)]);
    let (_, _) = run_check(root.path());

    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    let bytes = std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredItemTree)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let keys: Vec<[u8; 32]> = pack.entries.iter().map(|(key, _)| *key).collect();
    assert!(keys.contains(blake3::hash(source_a.as_bytes()).as_bytes()));
    assert!(keys.contains(blake3::hash(source_b.as_bytes()).as_bytes()));
    assert!(keys.is_sorted(), "entries are written in key order");

    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    assert_eq!(pack.entries.len(), 2, "one verdict per reported file");
}

/// The second run over an unchanged project serves and rewrites
/// nothing: its packs must decode to exactly the first run's.
#[test]
fn a_second_run_leaves_equivalent_packs_behind() {
    let root = project(&[("a.php", "<?php new Missing();")]);
    let (_, first_output) = run_check(root.path());
    let first_trees =
        std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let first_member_trees = std::fs::read(
        root.path()
            .join(".celerrate/cache/")
            .join(MEMBER_TREES_PACK),
    )
    .unwrap();
    let first_verdicts =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();

    let (_, second_output) = run_check(root.path());
    assert_eq!(first_output, second_output, "byte-identical rendering");
    assert_eq!(
        first_trees,
        std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap(),
    );
    assert_eq!(
        first_member_trees,
        std::fs::read(
            root.path()
                .join(".celerrate/cache/")
                .join(MEMBER_TREES_PACK)
        )
        .unwrap(),
    );
    assert_eq!(
        first_verdicts,
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap(),
    );
}

/// A planted entry whose record still holds but whose identifier the
/// binary no longer knows is exactly what `a_verdict_with_an_unknown_
/// identifier_is_discarded` above proves the pass recomputes rather than
/// serves. `persist` must mirror that refusal: it may not re-persist the
/// unknown-identifier verdict just because `validated_verdict` returned
/// it, or the honest recomputation the pass reported never reaches the
/// pack that seeds the next run.
#[test]
fn persist_does_not_re_persist_a_verdict_the_pass_refused_to_serve() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].id = "CEL9999".to_owned();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let (_, _) = run_check(root.path());

    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let (_, persisted) = pack.entries.iter().find(|(key, _)| *key == hash).unwrap();

    assert_eq!(persisted.diagnostics.len(), 1);
    let diagnostic = &persisted.diagnostics[0];
    assert_ne!(
        diagnostic.id, "CEL9999",
        "the discarded verdict must not survive into the persisted pack",
    );
    assert!(
        diagnostic.message.contains("Missing"),
        "the persisted diagnostic is the pass's honest recomputation: {diagnostic:?}",
    );
}

/// The spec's boundary, checkable on disk (audit finding I4): an
/// installed dependency is indexed — its item tree is in the pack,
/// which is what makes its symbols resolve on a warm start — but never
/// reported: no diagnostics entry may exist under its content hash.
#[test]
fn a_vendor_file_has_a_tree_entry_and_no_diagnostics_entry() {
    let vendor_source = "<?php namespace Lib; class Helper {}";
    let project_source = "<?php namespace App; use Lib\\Helper; new Helper();";
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("vendor/lib/src/Helper.php", vendor_source),
        (
            "vendor/composer/installed.json",
            r#"{"packages": [{"name": "acme/lib", "install-path": "../lib",
               "autoload": {"psr-4": {"Lib\\": "src/"}}}]}"#,
        ),
        ("src/App.php", project_source),
    ]);
    let (_, _) = run_check(root.path());

    // The expected header is derived from the project's own discovered
    // range, not hard-coded: `^8.2` maps to whatever maximum the binary
    // supports, and this test must not re-derive that rule.
    let session = Session::start(root.path());
    let header = PackHeader::current(
        session.configuration.php_version_range(&session.database),
        session.plugin_set_digest,
    );

    let vendor_hash = *blake3::hash(vendor_source.as_bytes()).as_bytes();
    let project_hash = *blake3::hash(project_source.as_bytes()).as_bytes();

    let bytes = std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let trees: Pack<Vec<([u8; 32], StoredItemTree)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let tree_keys: Vec<[u8; 32]> = trees.entries.iter().map(|(key, _)| *key).collect();
    assert!(
        tree_keys.contains(&vendor_hash),
        "the vendor file is indexed"
    );
    assert!(tree_keys.contains(&project_hash));

    let bytes = std::fs::read(
        root.path()
            .join(".celerrate/cache/")
            .join(MEMBER_TREES_PACK),
    )
    .unwrap();
    let member_trees: Pack<Vec<([u8; 32], StoredMemberTree)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let member_tree_keys: Vec<[u8; 32]> =
        member_trees.entries.iter().map(|(key, _)| *key).collect();
    assert!(
        member_tree_keys.contains(&vendor_hash),
        "the vendor file's member tree is indexed too"
    );
    assert!(member_tree_keys.contains(&project_hash));

    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let verdicts: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let verdict_keys: Vec<[u8; 32]> = verdicts.entries.iter().map(|(key, _)| *key).collect();
    assert!(
        !verdict_keys.contains(&vendor_hash),
        "an installed dependency never gets a diagnostics entry",
    );
    assert!(
        verdict_keys.contains(&project_hash),
        "the project file does"
    );
}

// ---------------------------------------------------------------------
// The checksum-valid adversarial matrix (audit finding I7): entries
// that pass the checksum but could not come from any real computation.
// This is the only hostile class that reaches the post-decode
// conversion code. The contract for every row: never a panic, never a
// user-visible internal error; entries that fail conversion recompute
// honestly.
// ---------------------------------------------------------------------

/// A span reaching past the file's end is discarded and the file
/// recomputed (pins the M4 fix at integration level).
#[test]
fn a_verdict_with_a_span_past_the_files_end_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].start = 100;
    verdict.diagnostics[0].end = 200;
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert!(outcome.panicked.is_empty(), "{:?}", outcome.panicked);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "planted by the cache probe"),
        "the oversized entry must not be served: {:?}",
        outcome.diagnostics,
    );
    assert_eq!(outcome.diagnostics.len(), 1, "recomputed honestly");
    assert!(outcome.diagnostics[0].message.contains("Missing"));
}

/// A stored tree whose declaration names an AST index no tree of this
/// file has. This row pins the end-to-end contract: a lying stored
/// declaration flows through the cache and the engine answers about it
/// without panicking and without an internal error. It does not pin the
/// unchecked-index guarantee itself — the traced execution path for this
/// input never reaches `AstIdMap::pointer` — that guarantee is pinned by
/// `celerrate_semantics`'s own `ast_id` unit tests and by the workspace-wide
/// `indexing_slicing` denial.
#[test]
fn an_item_tree_with_an_absurd_ast_index_never_panics() {
    let source = "<?php class Marker {} new Marker();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();

    let mut lying_tree = ItemTree::default();
    lying_tree.declarations.push(Declaration {
        kind: DeclarationKind::Class,
        name: "Marker".to_owned(),
        namespace: String::new(),
        ast_id: AstId {
            file: celerrate_source::FileId::new(0),
            index: u32::MAX,
        },
        extends: Vec::new(),
        implements: Vec::new(),
        trait_uses: Vec::new(),
    });
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_item_trees_pack(
        root.path(),
        &header,
        vec![(hash, StoredItemTree::of(&lying_tree))],
    );

    let (outcome, _) = run_check(root.path());
    assert_ne!(
        outcome,
        celerrate_cli::Outcome::InternalError,
        "an absurd AST index must never surface as an internal error",
    );
}

/// The member-tree sibling of the adversarial test above: a stored
/// class group names an AST index no tree of this file has. Pins the
/// same end-to-end contract for the member pack — a lying entry flows
/// through the engine (member lookup, linearization) without a panic
/// and without an internal error.
#[test]
fn a_member_tree_with_an_absurd_ast_index_never_panics() {
    let source = "<?php class Marker {} new Marker();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();

    let lying_tree = MemberTree {
        classes: vec![ClassMembers {
            kind: DeclarationKind::Class,
            name: Some("Marker".to_owned()),
            namespace: String::new(),
            ast_id: AstId {
                file: celerrate_source::FileId::new(0),
                index: u32::MAX,
            },
            docblock: None,
            members: Vec::new(),
            trait_uses: Vec::new(),
            attribute_names: Vec::new(),
            extends: Vec::new(),
            implements: Vec::new(),
        }],
        functions: Vec::new(),
    };
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_member_trees_pack(
        root.path(),
        &header,
        vec![(hash, StoredMemberTree::of(&lying_tree))],
    );

    let (outcome, _) = run_check(root.path());
    assert_ne!(
        outcome,
        celerrate_cli::Outcome::InternalError,
        "an absurd AST index must never surface as an internal error",
    );
}

/// Two entries under one content hash: the loader collects into a map,
/// so one wins; which one is not contractual. What is: no panic, no
/// internal error.
#[test]
fn duplicate_keys_in_a_pack_never_panic() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_item_trees_pack(
        root.path(),
        &header,
        vec![
            (hash, StoredItemTree::of(&ItemTree::default())),
            (hash, StoredItemTree::of(&parsed_marker_tree(source))),
        ],
    );

    let (outcome, _) = run_check(root.path());
    assert_ne!(outcome, celerrate_cli::Outcome::InternalError);
}

/// Builds the honest tree for `source`, so the duplicate above differs.
fn parsed_marker_tree(source: &str) -> ItemTree {
    let parse = celerrate_syntax::parse(source);
    ItemTree::from_root(celerrate_source::FileId::new(0), &parse.tree())
}

/// An entry with no records vacuously revalidates and its diagnostics
/// are served as-is. Within the accepted threat model (whoever writes
/// the pack controls the project), the contract is only no panic and
/// no internal error — plus the write-side invariant below, which is
/// why honest packs never look like this.
#[test]
fn an_empty_record_list_is_served_without_panicking() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.records = Vec::new();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let (outcome, _) = run_check(root.path());
    assert_ne!(outcome, celerrate_cli::Outcome::InternalError);
}

/// The write side of the invariant above: a persisted verdict for a
/// file that references names always carries revalidation records, so
/// an honest pack can never hit the vacuous-acceptance path.
#[test]
fn persist_records_every_referencing_files_lookups() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let (_, _) = run_check(root.path());

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let (_, persisted) = pack.entries.iter().find(|(key, _)| *key == hash).unwrap();
    assert!(
        !persisted.records.is_empty(),
        "a referencing file's verdict must carry its records",
    );
}

/// Audit finding I8: hit rate, revalidation acceptance, and persist
/// health were unobservable without a profiler. A warm session over an
/// unchanged project counts tree hits and served verdicts and nothing
/// discarded.
#[test]
fn a_warm_session_counts_tree_hits_and_served_verdicts() {
    use std::sync::atomic::Ordering;

    let root = project(&[("a.php", "<?php new Missing();")]);
    let (_, _) = run_check(root.path());

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);

    let statistics = &session.statistics;
    assert!(
        statistics.tree_hits.load(Ordering::Relaxed) >= 1,
        "the warm pass served at least one tree from the pack",
    );
    assert_eq!(statistics.verdicts_served.load(Ordering::Relaxed), 1);
    assert_eq!(statistics.verdicts_discarded.load(Ordering::Relaxed), 0);
    assert_eq!(statistics.verdicts_absent.load(Ordering::Relaxed), 0);
}

/// The cold side: no pack, everything misses and every verdict is
/// absent.
#[test]
fn a_cold_session_counts_misses_and_absences() {
    use std::sync::atomic::Ordering;

    let root = project(&[("a.php", "<?php new Missing();")]);
    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);

    let statistics = &session.statistics;
    assert!(statistics.tree_misses.load(Ordering::Relaxed) >= 1);
    assert_eq!(statistics.verdicts_absent.load(Ordering::Relaxed), 1);
    assert_eq!(statistics.verdicts_served.load(Ordering::Relaxed), 0);
}

/// Plan 9a, task 7: one inferred-signature entry per eligible body.
/// `a.php` carries a declared-return function and an unannotated one —
/// both get an entry, because the artifact is unconditional; only the
/// EDGES a declared return cuts, never the callee's own record. `b.php`
/// carries a trait a class `use`s: the trait's own method gets no
/// entry under the trait's key (decision 8's exclusion), the class's
/// own method does. `c.php` carries an anonymous class: its method
/// gets no entry either (no stable folded key for a caller elsewhere
/// to cite). `annotated` calls `plain` — no declared return exists for
/// `plain`, so the call resolves through the INFERRED tier, giving
/// `annotated`'s own entry a concrete `StoredInferredEdge` to check
/// alongside the plain declared-tier `functions` dependency `plain`
/// itself never has any of (it calls nothing).
#[test]
fn persist_writes_an_inferred_signature_entry_per_eligible_body() {
    let source_a = "<?php function plain() { return 'hello'; } function annotated(): int { return plain() === 'hello' ? 1 : 2; }";
    let source_b = "<?php trait Greets { public function greet() { return 'hi'; } } \
         class Greeter { use Greets; public function shout() { return 'HI'; } }";
    let source_c = "<?php function wrapper() { return new class { public function compute() { return 42; } }; }";
    let root = project(&[
        ("a.php", source_a),
        ("b.php", source_b),
        ("c.php", source_c),
    ]);
    let (_, _) = run_check(root.path());

    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    let bytes = std::fs::read(
        root.path()
            .join(".celerrate/cache/")
            .join(INFERRED_SIGNATURES_PACK),
    )
    .unwrap();
    let pack: Pack<Vec<(StoredSignatureKey, StoredInferredSignature)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();

    let entry = |key: &StoredSignatureKey| {
        pack.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, entry)| entry)
    };

    let plain_key = StoredSignatureKey::Function {
        key: "plain".to_owned(),
    };
    let annotated_key = StoredSignatureKey::Function {
        key: "annotated".to_owned(),
    };
    let shout_key = StoredSignatureKey::Method {
        class_key: "greeter".to_owned(),
        member_key: "shout".to_owned(),
    };
    let greet_key = StoredSignatureKey::Method {
        class_key: "greets".to_owned(),
        member_key: "greet".to_owned(),
    };

    let plain = entry(&plain_key).expect("the unannotated free function has an entry");
    let annotated = entry(&annotated_key).expect("the annotated free function still has one too");
    let shout = entry(&shout_key).expect("the class's own method has an entry");
    assert!(
        entry(&greet_key).is_none(),
        "no entry under the trait's own key",
    );
    assert!(
        pack.entries
            .iter()
            .all(|(key, _)| !matches!(key, StoredSignatureKey::Method { member_key, .. } if member_key == "compute")),
        "no entry for the anonymous class's method under any key",
    );

    assert_eq!(plain.content, *blake3::hash(source_a.as_bytes()).as_bytes());
    assert_eq!(
        annotated.content,
        *blake3::hash(source_a.as_bytes()).as_bytes()
    );
    assert_eq!(shout.content, *blake3::hash(source_b.as_bytes()).as_bytes());

    assert!(
        plain.classes.is_empty() && plain.functions.is_empty() && plain.inferred.is_empty(),
        "plain calls nothing and consults no class",
    );
    assert!(
        shout.classes.is_empty() && shout.functions.is_empty() && shout.inferred.is_empty(),
        "shout's body never references self/parent/static or a member",
    );
    assert_eq!(
        annotated.inferred,
        vec![celerrate_types::StoredInferredEdge {
            callee: plain_key,
            return_type: annotated.inferred[0].return_type.clone(),
        }],
        "annotated's call to the undeclared plain() resolves through the inferred tier",
    );
    assert!(
        annotated.classes.is_empty() && annotated.functions.is_empty(),
        "annotated consults no declared-tier callee and no class",
    );
}

/// Two identical runs in fresh directories produce byte-identical
/// `inferred_signatures.bin` — the same determinism contract the other
/// three packs already carry.
#[test]
fn the_signature_pack_is_sorted_and_deterministic() {
    let files: &[(&str, &str)] = &[
        (
            "a.php",
            "<?php function plain() { return 'hello'; } function annotated(): int { return 1; }",
        ),
        (
            "b.php",
            "<?php class Greeter { public function shout() { return 'HI'; } }",
        ),
    ];

    let first_root = project(files);
    run_check(first_root.path());
    let first = std::fs::read(
        first_root
            .path()
            .join(".celerrate/cache/")
            .join(INFERRED_SIGNATURES_PACK),
    )
    .unwrap();

    let second_root = project(files);
    run_check(second_root.path());
    let second = std::fs::read(
        second_root
            .path()
            .join(".celerrate/cache/")
            .join(INFERRED_SIGNATURES_PACK),
    )
    .unwrap();

    assert_eq!(first, second, "byte-identical across two fresh directories");

    let header = PackHeader::current(
        PhpVersionRange::point(PhpVersion::new(8, 5)),
        celerrate_cli::plugins::plugin_set_digest(),
    );
    let pack: Pack<Vec<(StoredSignatureKey, StoredInferredSignature)>> =
        celerrate_cli::cache::pack::decode(&first, &header).unwrap();
    let keys: Vec<&StoredSignatureKey> = pack.entries.iter().map(|(key, _)| key).collect();
    assert!(keys.is_sorted(), "entries are written in key order");
    assert_eq!(pack.entries.len(), 3, "plain, annotated, and shout");
}

/// A vendor file's own body must get NO signature entry: `analyze`
/// fans `inferred_body_types` over `inputs.reported` alone (the
/// project's own files — `analysis.rs`'s own rustdoc on
/// `AnalysisInputs::reported`), never over the whole `sources`/
/// `inputs.files` set the item-tree and member-tree packs cover. If
/// `collect_signature_entries` widened its own loop to that broader
/// set, it would force a FRESH interprocedural inference of every
/// vendor body the analysis pass never touched — this is exactly the
/// persist-may-only-read invariant (decision 8) the fourth pack must
/// not violate. The vendor class here (`Lib\Helper`) is referenced
/// from the project file, so its class surface IS consulted (through
/// `resolve_candidates`/name resolution), but its own method body is
/// never walked.
#[test]
fn a_vendor_files_body_has_no_signature_entry() {
    let vendor_source =
        "<?php namespace Lib; class Helper { public function compute() { return 1; } }";
    let project_source = "<?php namespace App; use Lib\\Helper; new Helper();";
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("vendor/lib/src/Helper.php", vendor_source),
        (
            "vendor/composer/installed.json",
            r#"{"packages": [{"name": "acme/lib", "install-path": "../lib",
               "autoload": {"psr-4": {"Lib\\": "src/"}}}]}"#,
        ),
        ("src/App.php", project_source),
    ]);
    let (_, _) = run_check(root.path());

    let session = Session::start(root.path());
    let header = PackHeader::current(
        session.configuration.php_version_range(&session.database),
        session.plugin_set_digest,
    );
    let bytes = std::fs::read(
        root.path()
            .join(".celerrate/cache/")
            .join(INFERRED_SIGNATURES_PACK),
    )
    .unwrap();
    let pack: Pack<Vec<(StoredSignatureKey, StoredInferredSignature)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();

    assert!(
        pack.entries.iter().all(|(key, _)| !matches!(
            key,
            StoredSignatureKey::Method { class_key, member_key }
                if class_key == "lib\\helper" && member_key == "compute"
        )),
        "the vendor method's body was never walked by the analysis pass, \
         so persist must not have forced a fresh inference of it",
    );
}

// ---------------------------------------------------------------------
// Plan 9a, task 8: the recursive memoized revalidation. Every scenario
// below builds a project, runs `check` once cold (persisting the fourth
// pack), mutates the project on disk exactly as the scenario names, then
// compares a WARM run's rendering (a fresh process, seeded from the
// persisted cache) against a genuinely FRESH run's rendering (an
// independent project directory holding the same final files, no cache
// at all). Byte-identical rendering across that boundary is the
// end-to-end proof that the served/recomputed answer is correct either
// way — the flagship early-cutoff property working across the process
// boundary, not merely in-process.
// ---------------------------------------------------------------------

/// One warm pass over `root`, exposing the session's own cache counters
/// alongside the rendered `check` output — `run_check` alone only
/// returns the rendered text, and the `signatures_found`/
/// `signatures_absent` instrument (task 8) is a `Session`-level counter,
/// not part of the rendering.
fn warm_signatures_found(root: &Path) -> u64 {
    use std::sync::atomic::Ordering;

    let session = Session::start(root);
    let _ = analyze(&session.inputs()).unwrap();
    session.statistics.signatures_found.load(Ordering::Relaxed)
}

/// A cold run over `initial`, then `edited` written over the same files
/// on disk before a second (warm) `check` run: the pattern every
/// scenario below shares. Returns the warm run's rendered output.
fn cold_then_warm(root: &Path, edited: &[(&str, &str)]) -> String {
    run_check(root);
    for (path, contents) in edited {
        std::fs::write(root.join(path), contents).unwrap();
    }
    let (_, warm_output) = run_check(root);
    warm_output
}

#[test]
fn a_warm_run_serves_inferred_returns_without_reinference() {
    let helper_source = "<?php function helper() { return 'not-an-int'; }";
    let caller_before =
        "<?php function takesInt(int $n): void {} function caller() { takesInt(helper()); }";
    let caller_after = "<?php function takesInt(int $n): void {} \
         function caller() { $noise = 1; takesInt(helper()); }";

    let root = project(&[("helper.php", helper_source), ("caller.php", caller_before)]);
    let warm_output = cold_then_warm(root.path(), &[("caller.php", caller_after)]);

    assert!(
        warm_signatures_found(root.path()) > 0,
        "the warm run must consult the persisted signature cache: \
         helper's callee return is served, not re-inferred",
    );

    let fresh_root = project(&[("helper.php", helper_source), ("caller.php", caller_after)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        warm_output, fresh_output,
        "warm rendering must equal a fresh recomputation over the same files",
    );
}

#[test]
fn an_edited_callee_with_an_unchanged_return_still_validates_the_caller() {
    // `helper`'s body changes (a fresh local variable), but its
    // inferred return stays the literal `1`: the early-cutoff property
    // across the process boundary, the design's flagship claim.
    let helper_before = "<?php function helper() { return 1; }";
    let helper_after = "<?php function helper() { $noise = 'x'; return 1; }";
    let caller_source =
        "<?php function takesInt(int $n): void {} function caller() { takesInt(helper()); }";

    let root = project(&[("helper.php", helper_before), ("caller.php", caller_source)]);
    let warm_output = cold_then_warm(root.path(), &[("helper.php", helper_after)]);

    assert!(
        warm_signatures_found(root.path()) > 0,
        "the warm run must consult the persisted signature cache",
    );

    let fresh_root = project(&[("helper.php", helper_after), ("caller.php", caller_source)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        warm_output, fresh_output,
        "the edited callee's unchanged return still validates the caller's \
         own record; the warm rendering matches a fresh recomputation",
    );
}

#[test]
fn a_changed_class_surface_invalidates_the_dependent_signature() {
    // A method added to `Box`, whose surface `useIt`'s own recorded
    // signature consulted (the receiver resolution for `$b->get()`):
    // the class-surface digest flips, so the dependent entry misses and
    // the run recomputes.
    let classes_before = "<?php class Box { public function get() { return 1; } }";
    let classes_after = "<?php class Box { \
         public function get() { return 1; } \
         public function extra() { return 2; } }";
    let caller_source = "<?php function useIt(Box $b): void { $b->get(); }";

    let root = project(&[
        ("classes.php", classes_before),
        ("caller.php", caller_source),
    ]);
    let warm_output = cold_then_warm(root.path(), &[("classes.php", classes_after)]);

    let fresh_root = project(&[
        ("classes.php", classes_after),
        ("caller.php", caller_source),
    ]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        warm_output, fresh_output,
        "a changed class surface must invalidate the dependent signature \
         and recompute, rendering identically to fresh",
    );
}

#[test]
fn a_cyclic_cluster_recomputes_and_stays_deterministic() {
    // Two mutually recursive functions with a real base case: the
    // recorded stance (decision 9) is that a cyclic cluster always
    // falls through to the fixpoint on a warm run — served or
    // recomputed is unspecified, but the rendering must equal fresh.
    let source = "<?php
        function evenOrOdd($n) {
            if ($n === 0) { return 'even'; }
            return oddOrEven($n - 1);
        }
        function oddOrEven($n) {
            if ($n === 0) { return 'odd'; }
            return evenOrOdd($n - 1);
        }
        function useBoth() { return evenOrOdd(3) . oddOrEven(3); }
    ";
    let root = project(&[("cycle.php", source)]);
    run_check(root.path());
    // No edit: a second, independently-started warm run over the exact
    // same files is still the scenario ("cached, warm run").
    let (_, first_warm) = run_check(root.path());
    let (_, second_warm) = run_check(root.path());
    assert_eq!(
        first_warm, second_warm,
        "two successive warm runs over an unchanged cyclic cluster agree",
    );

    let fresh_root = project(&[("cycle.php", source)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        first_warm, fresh_output,
        "a cyclic cluster's warm rendering equals a fresh recomputation",
    );
}

#[test]
fn a_never_returning_cycle_participant_never_validates_warm() {
    // `first` and `second` call each other with no non-throwing base
    // case: the whole cluster's fixpoint is `never`. Decision 9's
    // provisional-mismatch rule fires on every live demand inside this
    // cluster (a live `never` is always a mismatch, even against a
    // record that itself expects `never`), so neither participant ever
    // validates warm — both always fall through to real recomputation.
    // The observable here is termination plus byte-identical rendering,
    // across two independent entry points into the same cluster
    // (`first` and `second` are each analyzed as their own file, both
    // fanned out by the same `check` run) and across repeated warm
    // runs, never a stale served `never` widening a live join ascent.
    let first_source = "<?php
        function first(int $n) {
            if ($n <= 0) { throw new \\RuntimeException('boom'); }
            return second($n - 1);
        }
    ";
    let second_source = "<?php
        function second(int $n) {
            return first($n - 1);
        }
    ";
    let root = project(&[("first.php", first_source), ("second.php", second_source)]);
    run_check(root.path());
    let (_, first_warm) = run_check(root.path());
    let (_, second_warm) = run_check(root.path());
    assert_eq!(
        first_warm, second_warm,
        "repeated warm runs over the never-returning cluster stay deterministic",
    );

    let fresh_root = project(&[("first.php", first_source), ("second.php", second_source)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        first_warm, fresh_output,
        "the never-returning cluster's warm rendering equals fresh: no \
         stale served `never` ever widens the join ascent",
    );
}

#[test]
fn a_static_returning_callee_serves_warm() {
    // `Builder::make` is unannotated and returns `$this`: its inferred
    // return is the symbolic `static` placeholder (task 3's raw,
    // pre-substitution answer), never resolved against a receiver at
    // the walk site. The caller's own file is edited (a body change
    // that keeps its own diagnostics recomputable); `make`'s entry is
    // untouched and must validate and serve warm.
    let helper_source = "<?php class Builder { public function make() { return $this; } }";
    let caller_before = "<?php function useIt(Builder $b) { return $b->make(); }";
    let caller_after = "<?php function useIt(Builder $b) { $noise = 1; return $b->make(); }";

    let root = project(&[("helper.php", helper_source), ("caller.php", caller_before)]);
    let warm_output = cold_then_warm(root.path(), &[("caller.php", caller_after)]);

    assert!(
        warm_signatures_found(root.path()) > 0,
        "the warm run must consult the persisted signature cache: \
         make()'s raw `static` return is served, proving the task-3 \
         record carries the pre-substitution answer the live demand \
         (inferred_method_return) itself answers",
    );

    let fresh_root = project(&[("helper.php", helper_source), ("caller.php", caller_after)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(
        warm_output, fresh_output,
        "warm rendering must equal a fresh recomputation over the same files",
    );
}
