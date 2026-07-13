//! The cache snapshot seeds a fresh session. These tests hand-write
//! pack files and observe the session serving from them; the probe
//! entries deliberately violate the exactness contract, because a
//! correct entry is indistinguishable from a recomputation.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::analysis::analyze;
use celerrate_cli::cache::pack::{Pack, PackHeader, encode, write_atomically};
use celerrate_cli::cache::snapshot::{DIAGNOSTICS_PACK, ITEM_TREES_PACK};
use celerrate_cli::cache::stored::{
    StoredAnswer, StoredDiagnostic, StoredItemTree, StoredRecord, StoredSeverity, StoredSpace,
    StoredVerdict,
};
use celerrate_cli::session::Session;
use celerrate_project::{PhpVersion, PhpVersionRange};
use celerrate_semantics::{ItemTree, SymbolSpace, item_tree, source_symbol_table};

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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_item_trees_pack(root.path(), &header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    let tree = item_tree(&session.database, file);
    assert!(
        tree.declarations.is_empty(),
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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
    let other_header = PackHeader::current(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 2),
    ));
    write_item_trees_pack(root.path(), &other_header, vec![(hash, probe)]);

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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
fn a_completed_run_writes_both_packs_and_the_gitignore() {
    let root = project(&[("a.php", "<?php class A {} new Missing();")]);
    let (_, _) = run_check(root.path());

    let cache = root.path().join(".celerrate/cache");
    assert!(cache.join(ITEM_TREES_PACK).is_file());
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

    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
    let first_verdicts =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();

    let (_, second_output) = run_check(root.path());
    assert_eq!(first_output, second_output, "byte-identical rendering");
    assert_eq!(
        first_trees,
        std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap(),
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
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
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
