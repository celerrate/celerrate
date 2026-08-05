//! Presentation-time did-you-mean: computed at
//! render and fix time, for the reported diagnostics only, never
//! inside a memoized query. Nothing computed here is persisted: a
//! candidate goes stale the moment a nearer name appears, and no
//! revalidation record could keep it honest. Inside a phase query the
//! candidate search would also wire the global name set into every
//! file's dependency graph; here it wires into nothing.

use std::collections::{HashMap, HashSet, VecDeque};

use celerrate_db::{parse, source_text};
use celerrate_diagnostics::{Confidence, Diagnostic, Suggestion};
use celerrate_semantics::{
    ClassQuery, MemberKind, Reference, SymbolSpace, UseTables, collect_references,
    folded_member_key, folded_symbol_key, item_tree, linearized_class, resolve_candidates,
    source_symbol_table, stub_signature_table, stub_symbol_table,
};
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
use celerrate_stubs::StubMemberKind;

use crate::session::Session;

/// The three matrix rows `bounded_distance_pooled` works in, owned
/// outside the call so one allocation serves every candidate of a
/// pass. The profiling behind issue #124 attributed most of the
/// enrich phase to reallocating exactly these rows per candidate.
#[derive(Debug, Default)]
struct DistanceScratch {
    before_previous: Vec<usize>,
    previous: Vec<usize>,
    current: Vec<usize>,
}

/// The bounded edit distance over pre-lowercased characters and
/// caller-owned rows: the hot form every production caller goes
/// through. The length rejection lives here so no caller can forget
/// it.
fn bounded_distance_pooled(
    written: &[char],
    candidate: &[char],
    bound: usize,
    scratch: &mut DistanceScratch,
) -> Option<usize> {
    if written.len().abs_diff(candidate.len()) > bound {
        return None;
    }
    scratch.before_previous.clear();
    scratch.before_previous.extend(0..=candidate.len());
    scratch.previous.clear();
    scratch.previous.extend(0..=candidate.len());
    for (row, written_character) in written.iter().enumerate() {
        scratch.current.clear();
        scratch.current.push(row + 1);
        for (column, candidate_character) in candidate.iter().enumerate() {
            // The `get` fallbacks are unreachable (the rows are dense
            // by construction); they exist because indexing is denied
            // and a wrong answer here is caught by the tests anyway.
            let substitution = scratch
                .previous
                .get(column)
                .copied()
                .unwrap_or(usize::MAX - 1)
                + usize::from(written_character != candidate_character);
            let insertion = scratch
                .current
                .get(column)
                .copied()
                .unwrap_or(usize::MAX - 1)
                + 1;
            let deletion = scratch
                .previous
                .get(column + 1)
                .copied()
                .unwrap_or(usize::MAX - 1)
                + 1;
            let mut best = substitution.min(insertion).min(deletion);
            if row > 0 && column > 0 {
                let previous_written = written.get(row - 1);
                let previous_candidate = candidate.get(column - 1);
                if previous_written == Some(candidate_character)
                    && previous_candidate == Some(written_character)
                {
                    // Adjacent transposition: `..ab` -> `..ba` costs 1,
                    // read off the diagonal two rows up (dense by
                    // construction whenever `row > 0`).
                    let transposition = scratch
                        .before_previous
                        .get(column - 1)
                        .copied()
                        .unwrap_or(usize::MAX - 1)
                        + 1;
                    best = best.min(transposition);
                }
            }
            scratch.current.push(best);
        }
        if scratch.current.iter().min().copied().unwrap_or(0) > bound {
            return None;
        }
        // Rotates the three rows for the next iteration without
        // allocating: equivalent to `before_previous = previous; previous
        // = current`, but moving the buffers in place instead of cloning
        // them. The row left in `current` after both swaps is the stale
        // `before_previous` from two iterations back; it is `clear`ed and
        // rebuilt from scratch at the top of the loop before anything
        // reads it, so its leftover contents never leak into the result.
        std::mem::swap(&mut scratch.before_previous, &mut scratch.previous);
        std::mem::swap(&mut scratch.previous, &mut scratch.current);
    }
    scratch
        .previous
        .last()
        .copied()
        .filter(|&distance| distance <= bound)
}

/// Optimal string alignment distance (restricted Damerau-Levenshtein)
/// over lowercased characters, abandoned as soon as it provably
/// exceeds `bound`. A transposition of two adjacent characters costs 1
/// edit, not 2: transposition is the dominant typo class (`svae` for
/// `save`, `nmae` for `name`) and plain Levenshtein overcharges it,
/// pushing exactly the typos this feature exists for outside the
/// bound. Lowercasing makes a case-only typo distance 0, which is
/// exactly the fix the case-sensitive spaces (constants, properties,
/// enum cases) want suggested. Test-only: every production caller now
/// goes through the pooled, pre-lowercased form
/// (`bounded_distance_pooled`) with a shared scratch, so this
/// single-pair convenience form has no production caller left. It is
/// not an independent implementation — it lowers its inputs and then
/// calls `bounded_distance_pooled` itself, so it cannot catch a
/// regression in the pooled algorithm; see `did_you_mean`'s doc for
/// where this refactor's real behavior pin lives.
#[cfg(test)]
fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize> {
    let written: Vec<char> = written.to_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    bounded_distance_pooled(&written, &candidate, bound, &mut DistanceScratch::default())
}

/// The "bounded edit distance" bound: tight for short
/// names (almost anything is within 2 of a 3-letter name), 2 otherwise.
fn distance_bound(name: &str) -> usize {
    if name.chars().count() <= 4 { 1 } else { 2 }
}

/// The ambiguity discipline: a unique
/// minimal-distance candidate becomes an applicable suggestion; a tie
/// is listed in a note instead, because bulk `--fix-suggestions` must
/// never apply a guess the engine itself knows is ambiguous.
#[derive(Debug, PartialEq, Eq)]
enum DidYouMean {
    Nothing,
    Unique(String),
    Tie(Vec<String>),
}

/// One pool name with everything the per-candidate loop needs,
/// computed once at pool construction: the fold key the exclusion
/// compares, and the lowercased characters the distance walks. Before
/// this existed, both were recomputed for the whole pool on every
/// diagnostic — the dominant cost of the enrich phase (issue #124).
struct PoolEntry {
    original: String,
    folded: String,
    lowercase: Vec<char>,
}

/// Builds a pool from declared names and the space's fold function.
fn pool_entries(names: Vec<String>, fold: impl Fn(&str) -> String) -> Vec<PoolEntry> {
    names
        .into_iter()
        .map(|name| PoolEntry {
            folded: fold(&name),
            lowercase: name.to_lowercase().chars().collect(),
            original: name,
        })
        .collect()
}

/// The pooled did-you-mean search: no clone, no re-fold, one scratch
/// for every candidate. `excluded_folded` is the per-diagnostic part of
/// an otherwise shared pool: a name folding equal to the attempted key
/// would have resolved, so it is skipped inline. Answers the outcome
/// and its minimal distance (`None` exactly when the outcome is
/// `Nothing`).
fn did_you_mean_pooled(
    written: &str,
    pool: &[PoolEntry],
    excluded_folded: Option<&str>,
    scratch: &mut DistanceScratch,
) -> (DidYouMean, Option<usize>) {
    let bound = distance_bound(written);
    let written_lowercase: Vec<char> = written.to_lowercase().chars().collect();
    let mut minimum: Option<usize> = None;
    let mut names: Vec<&str> = Vec::new();
    for entry in pool {
        if excluded_folded == Some(entry.folded.as_str()) {
            continue;
        }
        let Some(distance) =
            bounded_distance_pooled(&written_lowercase, &entry.lowercase, bound, scratch)
        else {
            continue;
        };
        match minimum {
            Some(best) if distance > best => {}
            Some(best) if distance == best => {
                if !names.contains(&entry.original.as_str()) {
                    names.push(&entry.original);
                }
            }
            _ => {
                minimum = Some(distance);
                names = vec![&entry.original];
            }
        }
    }
    names.sort_unstable();
    let outcome = match names.len() {
        0 => DidYouMean::Nothing,
        1 => names.pop().map_or(DidYouMean::Nothing, |name| {
            DidYouMean::Unique(name.to_owned())
        }),
        _ => DidYouMean::Tie(names.into_iter().map(str::to_owned).collect()),
    };
    (outcome, minimum)
}

/// The owned-vector test helper for `did_you_mean_pooled`: builds a
/// throwaway pool and a fresh scratch, then delegates. Test-only: every
/// production caller builds its pool once per pass and calls
/// `did_you_mean_pooled` directly with the shared scratch. This is
/// *not* an independent reference implementation — it bottoms out in
/// the very algorithm it is compared against below, so it cannot catch
/// a regression there; it exists to let a test hand in a plain
/// `Vec<String>` and to exercise scratch-reuse safety (see the tests
/// using it). The real behavior pin for this refactor is the suite of
/// fixture-driven enrichment tests further down this module (starting
/// with `an_unknown_class_with_one_near_declaration_gains_an_applicable_suggestion`),
/// which run the pooled path end-to-end against real diagnostics and
/// assert the exact suggestion and note text produced.
#[cfg(test)]
fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean {
    let pool = pool_entries(candidates, |name| name.to_owned());
    did_you_mean_pooled(written, &pool, None, &mut DistanceScratch::default()).0
}

/// The last segment of a qualified name: `Lib\Client` -> `Client`.
fn terminal_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// Everything before a qualified name's last segment: `Lib\Sub\Client`
/// -> `Lib\Sub`, `Client` -> `""`. The namespace prefix guard 2
/// compares: a candidate sharing it with the attempted key differs
/// only in its terminal segment, so rewriting the terminal segment of
/// the written name is safe.
fn qualifier(name: &str) -> &str {
    name.rfind('\\')
        .and_then(|index| name.get(..index))
        .unwrap_or("")
}

/// What one diagnostic gains: an applicable suggestion, or a note when
/// the engine itself knows the guess is ambiguous or unsafe to apply
/// blindly.
enum Enrichment {
    Suggestion(Suggestion),
    Note(String),
}

/// Adds presentation-time did-you-mean suggestions and notes to the
/// reported diagnostics. Pure presentation: the input's length and
/// order are preserved, the persisted verdicts never see the result,
/// and nothing here runs inside a salsa query.
pub fn enrich(session: &Session, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    let mut pools = CandidatePools::new(session);
    diagnostics
        .iter()
        .map(|diagnostic| enrich_one(session, &mut pools, diagnostic.clone()))
        .collect()
}

fn enrich_one(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    mut diagnostic: Diagnostic,
) -> Diagnostic {
    let Some((file, range)) = diagnostic.span() else {
        return diagnostic;
    };
    // Matching on the code string is deliberate: the identifiers are
    // the frozen public contract, and the CLI must not depend on which
    // crate happens to declare each constant.
    let enrichment = match diagnostic.id.as_str() {
        "CEL0018" => symbol_did_you_mean(session, pools, file, range, SymbolSpace::ClassLike),
        "CEL0019" => symbol_did_you_mean(session, pools, file, range, SymbolSpace::Function),
        "CEL0020" => symbol_did_you_mean(session, pools, file, range, SymbolSpace::Constant),
        "CEL0030" => member_did_you_mean(
            session,
            pools,
            file,
            range,
            &diagnostic.message,
            MemberKind::Method,
        ),
        "CEL0031" => member_did_you_mean(
            session,
            pools,
            file,
            range,
            &diagnostic.message,
            MemberKind::Property,
        ),
        "CEL0032" => member_did_you_mean(
            session,
            pools,
            file,
            range,
            &diagnostic.message,
            MemberKind::ClassConstant,
        ),
        "CEL0033" => member_did_you_mean(
            session,
            pools,
            file,
            range,
            &diagnostic.message,
            MemberKind::EnumCase,
        ),
        _ => None,
    };
    match enrichment {
        Some(Enrichment::Suggestion(suggestion)) => diagnostic.suggestions.push(suggestion),
        Some(Enrichment::Note(note)) => diagnostic.notes.push(note),
        None => {}
    }
    diagnostic
}

/// The source text under a span, from the exact decoded input the
/// analysis read.
fn span_text(session: &Session, file: FileId, range: TextRange) -> Option<String> {
    let source = session.sources.get(&file)?;
    let text = source_text(&session.database, *source).as_ref().ok()?;
    text.text()
        .get(usize::from(range.start())..usize::from(range.end()))
        .map(str::to_owned)
}

/// The declared fully qualified names of one symbol space, source and
/// stub tables combined, built at most once per [`enrich`] call and
/// shared across every diagnostic in that space. With a misconfigured
/// autoload there can be thousands of unknown-symbol diagnostics,
/// which is exactly when this tool runs: rebuilding, sorting, and
/// deduplicating the whole version-filtered stub table per diagnostic
/// would be wasted work on the case that matters most.
struct CandidatePools<'a> {
    session: &'a Session,
    classes: Option<Vec<PoolEntry>>,
    functions: Option<Vec<PoolEntry>>,
    constants: Option<Vec<PoolEntry>>,
    /// The receiver's member names of one kind, keyed by (resolved
    /// class-like key, kind), built at most once per [`enrich`] call.
    /// Excludes nothing yet: the written member's own key is filtered
    /// out per diagnostic afterwards, since two diagnostics can share a
    /// receiver while writing different unknown members.
    members: HashMap<(String, MemberKind), Vec<PoolEntry>>,
    /// One file's statically named references, parsed and collected on
    /// first use per file and shared with every later call in the same
    /// pass. `attempted_keys` and `resolved_receiver_key` both used to
    /// call `collect_references` themselves, once per diagnostic: a
    /// misconfigured autoload can carry thousands of unknown-symbol or
    /// unknown-member diagnostics against one file, which turned into
    /// thousands of tree walks over that file. Caching here turns it
    /// into at most one walk per file for the whole pass.
    reference_cache: HashMap<FileId, Vec<Reference>>,
    /// The pass-wide distance rows, shared by every diagnostic.
    scratch: DistanceScratch,
}

impl<'a> CandidatePools<'a> {
    fn new(session: &'a Session) -> Self {
        Self {
            session,
            classes: None,
            functions: None,
            constants: None,
            members: HashMap::new(),
            reference_cache: HashMap::new(),
            scratch: DistanceScratch::default(),
        }
    }

    /// The file's statically named references, computed on first use
    /// per file and shared with every later call in the same pass. An
    /// unparseable or no-longer-tracked file yields an empty list, the
    /// same "nothing found" outcome a failed lookup would have produced
    /// before this cache existed.
    fn references(&mut self, file: FileId) -> &[Reference] {
        let session = self.session;
        self.reference_cache
            .entry(file)
            .or_insert_with(|| match session.sources.get(&file) {
                Some(&source) => {
                    let root = parse(&session.database, source).tree();
                    collect_references(&root)
                }
                None => Vec::new(),
            })
            .as_slice()
    }

    /// The declared pool of `space` plus the shared distance scratch,
    /// split-borrowed so one call feeds `did_you_mean_pooled` directly.
    fn symbol_pool(&mut self, space: SymbolSpace) -> (&[PoolEntry], &mut DistanceScratch) {
        let session = self.session;
        let slot = match space {
            SymbolSpace::ClassLike => &mut self.classes,
            SymbolSpace::Function => &mut self.functions,
            SymbolSpace::Constant => &mut self.constants,
        };
        let entries = slot.get_or_insert_with(|| {
            pool_entries(declared_pool(session, space), |name| {
                folded_symbol_key(space, name)
            })
        });
        (entries.as_slice(), &mut self.scratch)
    }

    /// The member pool of (`class_key`, `kind`) plus the shared
    /// scratch, same split-borrow shape as `symbol_pool`. `class_key`
    /// must already be the resolved fully qualified key (see
    /// `receiver_class_key`), not the as-written receiver text.
    fn member_pool(
        &mut self,
        class_key: &str,
        kind: MemberKind,
    ) -> (&[PoolEntry], &mut DistanceScratch) {
        let session = self.session;
        let entries = self
            .members
            .entry((class_key.to_owned(), kind))
            .or_insert_with(|| {
                pool_entries(member_candidates(session, class_key, kind), |name| {
                    folded_member_key(kind, name)
                })
            });
        (entries.as_slice(), &mut self.scratch)
    }
}

/// Every declared qualified name of `space`, source and stub halves
/// alike. Unlike the old terminal-segment pool, the qualified name is
/// kept whole: comparing keys rather than bare terminal segments is
/// the whole point of this design.
fn declared_pool(session: &Session, space: SymbolSpace) -> Vec<String> {
    let db = &session.database;
    let mut names: Vec<String> = Vec::new();
    for entry in source_symbol_table(db, session.files).entries() {
        if entry.space == space {
            names.push(entry.original.clone());
        }
    }
    for entry in stub_symbol_table(db, session.stubs, session.configuration).entries() {
        if entry.space == space {
            names.push(entry.symbol.name.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The fully qualified names the reference actually tried, in PHP's
/// resolution order. The rule that reported the diagnostic keeps only
/// the written name (`ResolutionOutcome::Unresolved` is a unit
/// variant), so this recomputes the attempt from public API: the
/// file's item tree, the namespace covering the diagnostic's span (as
/// `collect_references` walks it), the `use` tables of that namespace,
/// and `resolve_candidates`. Mirrors the walk in
/// `celerrate_semantics::reference_checks`. The reference itself comes
/// from `pools`' per-file cache rather than a fresh `collect_references`
/// call, so a file carrying many of these diagnostics pays the walk at
/// most once.
fn attempted_keys(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    file: FileId,
    range: TextRange,
    space: SymbolSpace,
) -> Option<Vec<String>> {
    let source = *session.sources.get(&file)?;
    let reference = pools
        .references(file)
        .iter()
        .find(|reference| reference.range == range && reference.space == space)?
        .clone();
    let tree = item_tree(&session.database, source);
    let tables = UseTables::for_namespace(tree, &reference.namespace);
    Some(resolve_candidates(
        &reference.written,
        space,
        &reference.namespace,
        &tables,
    ))
}

/// Runs `did_you_mean_pooled` once per attempted key (PHP tries more
/// than one only for the function/constant global fallback), against
/// the pool with that key's own fold-equal entries excluded (a name folding
/// equal to an attempted key would have resolved, so excluding it is
/// the per-diagnostic part of an otherwise shared pool). Returns the
/// attempted key with the nearest outcome and that outcome; on an
/// exact tie between two attempted keys the first in resolution order
/// wins, which is PHP's own precedence.
fn did_you_mean_across_keys(
    attempted: Vec<String>,
    pool: &[PoolEntry],
    space: SymbolSpace,
    scratch: &mut DistanceScratch,
) -> Option<(String, DidYouMean)> {
    let mut best: Option<(String, DidYouMean, usize)> = None;
    for key in attempted {
        let folded_key = folded_symbol_key(space, &key);
        let (outcome, distance) =
            did_you_mean_pooled(&key, pool, Some(folded_key.as_str()), scratch);
        let Some(distance) = distance else { continue };
        let replace = match &best {
            Some((_, _, best_distance)) => distance < *best_distance,
            None => true,
        };
        if replace {
            best = Some((key, outcome, distance));
        }
    }
    best.map(|(key, outcome, _)| (key, outcome))
}

fn symbol_did_you_mean(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    file: FileId,
    range: TextRange,
    space: SymbolSpace,
) -> Option<Enrichment> {
    let written = span_text(session, file, range)?;
    let attempted = attempted_keys(session, pools, file, range, space)?;
    let (pool, scratch) = pools.symbol_pool(space);
    let (winning_key, outcome) = did_you_mean_across_keys(attempted, pool, space, scratch)?;
    match outcome {
        DidYouMean::Nothing => None,
        DidYouMean::Unique(candidate) => {
            // Guard 1: an alias rewrites the written terminal to
            // something that does not share the resolved terminal
            // (`use Lib\Missing as M;` writes `M`, resolves to
            // `Missing`); editing `M` to a `Missing`-shaped name would
            // not resolve.
            let guard_one = terminal_segment(&written) == terminal_segment(&winning_key);
            // Guard 2: the winning declared key differs from the
            // attempted key only in its terminal segment; otherwise
            // the edit would move the reference into a different
            // namespace that still does not resolve.
            let guard_two = qualifier(&candidate) == qualifier(&winning_key);
            if guard_one && guard_two {
                let candidate_terminal = terminal_segment(&candidate).to_owned();
                let prefix_length =
                    u32::try_from(written.rfind('\\').map_or(0, |index| index + 1)).ok()?;
                let edit_range =
                    TextRange::new(range.start() + TextSize::from(prefix_length), range.end());
                Some(Enrichment::Suggestion(Suggestion {
                    message: format!("did you mean `{candidate_terminal}`?"),
                    confidence: Confidence::NeedsReview,
                    edits: vec![TextEdit {
                        file,
                        range: edit_range,
                        replacement: candidate_terminal,
                    }],
                }))
            } else {
                Some(Enrichment::Note(format!("did you mean `{candidate}`?")))
            }
        }
        DidYouMean::Tie(names) => Some(Enrichment::Note(tie_note(&names))),
    }
}

/// The note a tied `DidYouMean::Tie` outcome renders as: every tied
/// name, backtick-wrapped, comma-joined. Shared by every family whose
/// resolution can tie, so the rendered wording stays byte-identical
/// wherever it appears.
fn tie_note(names: &[String]) -> String {
    let listed = names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("did you mean one of {listed}?")
}

/// Extracts the two backticked operands of the pinned member-message
/// shapes (`` unknown method `m` on `T` ``). The message is the stored
/// form of the diagnostic — on a warm run the structured verdict no
/// longer exists — so the message is the interface, and a test pins
/// this parser against the emitters' formats.
fn parse_member_message(message: &str) -> Option<(String, String)> {
    let mut segments = message.split('`');
    let _head = segments.next()?;
    let member = segments.next()?;
    let _middle = segments.next()?;
    let receiver = segments.next()?;
    let member = member.strip_prefix('$').unwrap_or(member);
    if member.is_empty() || receiver.is_empty() {
        return None;
    }
    Some((member.to_owned(), receiver.to_owned()))
}

/// The member-name token inside the diagnostic's span: the last
/// occurrence of the written member that is preceded by `->`, `?->`,
/// or `::` (whitespace allowed in between) and ends at a word
/// boundary. `None` skips the applicable edit rather than guessing.
fn member_token_range(span_text: &str, member: &str, span_start: TextSize) -> Option<TextRange> {
    let mut search_end = span_text.len();
    loop {
        let position = span_text.get(..search_end)?.rfind(member)?;
        let before = span_text.get(..position)?.trim_end();
        let after = span_text.get(position + member.len()..)?;
        let boundary_after = after
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        if boundary_after && (before.ends_with("->") || before.ends_with("::")) {
            let start = span_start + TextSize::from(u32::try_from(position).ok()?);
            let length = TextSize::from(u32::try_from(member.len()).ok()?);
            return Some(TextRange::new(start, start + length));
        }
        if position == 0 {
            return None;
        }
        search_end = position;
    }
}

/// One member-family diagnostic's enrichment: the receiver's as-written
/// text is resolved to its actual class-like key first (see
/// `receiver_class_key`), the pool for that key comes from the shared
/// per-(class_key, kind) cache, the written member's own key is
/// excluded from it, and a unique winner becomes an applicable edit
/// only when its token can actually be located in the span — a member
/// reference has no namespace or alias dimension once the receiver's
/// class is known, so (unlike the symbol families) no further guard is
/// needed once the member key matches.
fn member_did_you_mean(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    file: FileId,
    range: TextRange,
    message: &str,
    kind: MemberKind,
) -> Option<Enrichment> {
    let (member, receiver) = parse_member_message(message)?;
    // A display the folded key cannot round-trip (a union type, an
    // intersection type, an anonymous class) yields no candidates and
    // therefore no noise.
    if receiver.contains('|') || receiver.contains('&') || receiver.contains('@') {
        return None;
    }
    let class_key = receiver_class_key(session, pools, file, range, &receiver);
    let written_key = folded_member_key(kind, &member);
    let (pool, scratch) = pools.member_pool(&class_key, kind);
    let (outcome, _) = did_you_mean_pooled(&member, pool, Some(written_key.as_str()), scratch);
    match outcome {
        DidYouMean::Nothing => None,
        DidYouMean::Unique(candidate) => {
            // The span is only decoded here, the one arm that needs it:
            // an undecodable span must not suppress the `Tie` and
            // token-not-found notes below, which need no source text.
            let text = span_text(session, file, range)?;
            match member_token_range(&text, &member, range.start()) {
                Some(edit_range) => Some(Enrichment::Suggestion(Suggestion {
                    message: format!("did you mean `{candidate}`?"),
                    confidence: Confidence::NeedsReview,
                    edits: vec![TextEdit {
                        file,
                        range: edit_range,
                        replacement: candidate,
                    }],
                })),
                // A unique candidate without a locatable token degrades to
                // a note: an applicable edit is never guessed.
                None => Some(Enrichment::Note(format!("did you mean `{candidate}`?"))),
            }
        }
        DidYouMean::Tie(names) => Some(Enrichment::Note(tie_note(&names))),
    }
}

/// The receiver's fully qualified class-like key. A scoped access
/// (`Foo::CONST`, `Foo::method()`) reports the as-written subject text
/// verbatim (`written_class_display`, kept for message legibility): a
/// bare name resolves in PHP to the current namespace only, with no
/// global fallback, so the naive fold of that bare text can collide
/// with an unrelated same-named class that happens to live in the
/// global namespace (a hand-written global class, or any
/// phpstorm-stub class, which are all global) even though PHP itself
/// would never look there. Resolving the file's own class-like
/// reference at this span through its namespace and `use` tables is
/// therefore tried first, exactly like `attempted_keys` does for the
/// unknown-symbol families; only when that resolution yields nothing
/// does the bare fold get trusted. Instance access (`$value->member`)
/// reports the already-resolved key (`receiver_display` in
/// `celerrate_types::checks::receivers`), and no class-like reference
/// written that way sits inside the diagnostic's span, so
/// `resolved_receiver_key` always answers `None` there and the fold --
/// already correct in that case -- is what actually gets used; the
/// instance path still looks through `pools`' per-file reference cache
/// on the way to that `None`, but since the cache is shared and built
/// at most once per file, it no longer pays a fresh syntax-tree walk
/// for every instance-access diagnostic, only the one shared walk the
/// first diagnostic in that file paid for anyone. One escalation order
/// stays correct for both access shapes rather than two paths that
/// could drift apart. Falling back
/// to the folded written text when resolution fails is safe either
/// way: an unresolvable key simply yields no candidates below, which
/// is the same "no enrichment" outcome as returning `None` here would
/// have produced.
fn receiver_class_key(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    file: FileId,
    range: TextRange,
    receiver: &str,
) -> String {
    resolved_receiver_key(session, pools, file, range, receiver)
        .unwrap_or_else(|| folded_symbol_key(SymbolSpace::ClassLike, receiver))
}

/// The fully qualified key a written class-like reference resolves to,
/// recomputed from the file's own syntax tree: the class-like
/// reference whose written text matches `receiver` and whose range
/// falls inside the diagnostic's span (the scoped subject is a strict
/// sub-range of the whole scoped-access expression the diagnostic
/// anchors to), resolved through its own namespace and `use` tables —
/// mirrors the walk in `celerrate_types::checks::members::resolve_scoped_class_key`.
fn resolved_receiver_key(
    session: &Session,
    pools: &mut CandidatePools<'_>,
    file: FileId,
    range: TextRange,
    receiver: &str,
) -> Option<String> {
    let source = *session.sources.get(&file)?;
    let reference = pools
        .references(file)
        .iter()
        .find(|reference| {
            reference.space == SymbolSpace::ClassLike
                && reference.written == receiver
                && range.contains_range(reference.range)
        })?
        .clone();
    let tree = item_tree(&session.database, source);
    let tables = UseTables::for_namespace(tree, &reference.namespace);
    let attempted = resolve_candidates(
        &reference.written,
        SymbolSpace::ClassLike,
        &reference.namespace,
        &tables,
    )
    .into_iter()
    .next()?;
    // `resolve_candidates` answers the fully qualified spelling, not
    // the folded key `ClassQuery` and the stub table expect.
    Some(folded_symbol_key(SymbolSpace::ClassLike, &attempted))
}

/// The class-like's member surface of the queried kind: the linearized
/// source surface first, then the compiled stub graph behind its stub
/// edges (or from the key itself when the class is no source class),
/// breadth-first over parent links exactly like `lookup_member`'s stub
/// walk. Virtual (annotation-declared) members are deliberately not in
/// the pool here. `class_key` must already be resolved
/// (see `receiver_class_key`): a display the folded key cannot
/// round-trip (a union type, an anonymous class) is filtered out
/// there, never reaching here.
fn member_candidates(session: &Session, class_key: &str, kind: MemberKind) -> Vec<String> {
    let db = &session.database;
    let stub_kind = match kind {
        MemberKind::Method => StubMemberKind::Method,
        MemberKind::Property => StubMemberKind::Property,
        MemberKind::ClassConstant => StubMemberKind::ClassConstant,
        MemberKind::EnumCase => StubMemberKind::EnumCase,
    };
    let mut names: Vec<String> = Vec::new();
    let mut stub_roots: Vec<String> = Vec::new();
    let class = ClassQuery::new(db, class_key.to_owned());
    match linearized_class(
        db,
        session.files,
        session.stubs,
        session.configuration,
        class,
    )
    .as_ref()
    {
        Some(linearized) => {
            for entry in &linearized.members {
                if entry.member.kind == kind {
                    names.push(entry.member.name.clone());
                }
            }
            for edge in &linearized.ancestry {
                if let Some(stub_key) = &edge.stub {
                    stub_roots.push(stub_key.clone());
                }
            }
        }
        None => stub_roots.push(class_key.to_owned()),
    }
    let table = stub_signature_table(db, session.stubs);
    let range = session.configuration.php_version_range(db);
    let mut queue: VecDeque<String> = stub_roots.into();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(key) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let Some(surface) = table.class(&key) else {
            continue;
        };
        for member in &surface.members {
            if member.kind == stub_kind && member.availability.exists_in(range) {
                names.push(member.name.clone());
            }
        }
        for parent in &surface.parents {
            queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
        }
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::{Confidence, Diagnostic};

    use super::{DidYouMean, bounded_distance, did_you_mean, distance_bound, terminal_segment};
    use crate::analysis;
    use crate::session::Session;

    #[test]
    fn the_distance_is_optimal_string_alignment_over_lowercased_names() {
        assert_eq!(bounded_distance("svae", "save", 2), Some(1));
        assert_eq!(bounded_distance("nmae", "name", 2), Some(1));
        assert_eq!(bounded_distance("save", "save", 2), Some(0));
        assert_eq!(bounded_distance("php_eol", "PHP_EOL", 2), Some(0));
        assert_eq!(bounded_distance("Activ", "Active", 2), Some(1));
    }

    #[test]
    fn a_distance_beyond_the_bound_is_none_not_a_number() {
        assert_eq!(bounded_distance("draft", "active", 2), None);
        assert_eq!(bounded_distance("a", "abcd", 2), None);
    }

    #[test]
    fn the_bound_is_one_for_short_names_and_two_otherwise() {
        assert_eq!(distance_bound("save"), 1);
        assert_eq!(distance_bound("saved"), 2);
        assert_eq!(distance_bound("é"), 1, "characters, not bytes");
    }

    #[test]
    fn a_unique_minimal_candidate_wins() {
        let outcome = did_you_mean(
            "svae",
            vec!["save".to_owned(), "wave".to_owned(), "unrelated".to_owned()],
        );
        // `svae` -> `save` is 1 (adjacent transposition); `svae` -> `wave`
        // is 2, outside the bound of 1: `save` wins uniquely.
        assert_eq!(outcome, DidYouMean::Unique("save".to_owned()));
        let outcome = did_you_mean("Activ", vec!["Active".to_owned(), "Passive".to_owned()]);
        assert_eq!(outcome, DidYouMean::Unique("Active".to_owned()));
    }

    #[test]
    fn a_nearer_candidate_replaces_a_farther_one_whatever_the_order() {
        let forward = did_you_mean("sive", vec!["salve".to_owned(), "save".to_owned()]);
        let backward = did_you_mean("sive", vec!["save".to_owned(), "salve".to_owned()]);
        assert_eq!(forward, DidYouMean::Unique("save".to_owned()));
        assert_eq!(forward, backward);
    }

    #[test]
    fn tied_candidates_are_sorted_and_deduplicated() {
        let outcome = did_you_mean(
            "sive",
            vec!["sove".to_owned(), "save".to_owned(), "sove".to_owned()],
        );
        assert_eq!(
            outcome,
            DidYouMean::Tie(vec!["save".to_owned(), "sove".to_owned()]),
        );
    }

    #[test]
    fn no_candidate_in_bound_is_nothing() {
        assert_eq!(
            did_you_mean("svae", vec!["unrelated".to_owned()]),
            DidYouMean::Nothing,
        );
        assert_eq!(did_you_mean("svae", Vec::new()), DidYouMean::Nothing);
    }

    #[test]
    fn the_terminal_segment_is_the_name_after_the_last_backslash() {
        assert_eq!(terminal_segment("Lib\\Client"), "Client");
        assert_eq!(terminal_segment("Client"), "Client");
        assert_eq!(terminal_segment("\\App\\Http\\Kernel"), "Kernel");
    }

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

    /// Analyzes a fixture project and enriches its report, exactly as
    /// the single-pass path will.
    fn enriched(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<Diagnostic>) {
        let root = project(files);
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let outcome = analysis::analyze(&inputs).unwrap_or_default();
        let enriched = super::enrich(&session, &outcome.diagnostics);
        (root, enriched)
    }

    const MANIFEST: &str =
        r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

    /// Adds a `classmap` root over a single root-level file, alongside
    /// the usual `App\` psr-4 mapping, so a fixture can also declare a
    /// genuinely global-namespace class that gets discovered and
    /// analyzed (not just `src/`-namespaced ones).
    const MANIFEST_WITH_LEGACY: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}, "classmap": ["legacy.php"]}}"#;

    #[test]
    fn an_unknown_class_with_one_near_declaration_gains_an_applicable_suggestion() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew PaymentGatewya();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.id.as_str(), "CEL0018");
        assert_eq!(diagnostic.suggestions.len(), 1);
        let suggestion = &diagnostic.suggestions[0];
        assert_eq!(suggestion.message, "did you mean `PaymentGateway`?");
        assert_eq!(suggestion.confidence, Confidence::NeedsReview);
        assert_eq!(suggestion.edits.len(), 1);
        // The edit replaces exactly the written name, in the
        // diagnostic's own file.
        let (file, range) = diagnostic.span().unwrap();
        assert_eq!(suggestion.edits[0].file, file);
        assert_eq!(suggestion.edits[0].range, range);
        assert_eq!(suggestion.edits[0].replacement, "PaymentGateway");
    }

    #[test]
    fn a_qualified_spelling_keeps_its_prefix_and_replaces_the_terminal_segment_only() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App\\Billing;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Billing\\PaymentGatewya();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let suggestion = &diagnostics[0].suggestions[0];
        // The span covers `Billing\PaymentGatewya`; the edit covers
        // only `PaymentGatewya`.
        let (_, span) = diagnostics[0].span().unwrap();
        assert!(suggestion.edits[0].range.start() > span.start());
        assert_eq!(suggestion.edits[0].range.end(), span.end());
        assert_eq!(suggestion.edits[0].replacement, "PaymentGateway");
    }

    #[test]
    fn a_case_only_constant_typo_suggests_the_declared_spelling() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/constants.php",
                "<?php\ndefine('DATABASE_TIMEOUT_LIMIT', 30);\necho database_timeout_limit;\n",
            ),
        ]);
        let constant: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0020")
            .collect();
        assert_eq!(constant.len(), 1);
        assert_eq!(constant[0].suggestions.len(), 1);
        let suggestion = &constant[0].suggestions[0];
        assert_eq!(suggestion.message, "did you mean `DATABASE_TIMEOUT_LIMIT`?");
        assert_eq!(suggestion.edits.len(), 1);
        assert_eq!(suggestion.edits[0].replacement, "DATABASE_TIMEOUT_LIMIT");
    }

    #[test]
    fn a_diagnostic_with_no_near_candidate_is_returned_untouched() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew CompletelyUnheardOfThing();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].suggestions.is_empty());
        assert!(diagnostics[0].notes.is_empty());
    }

    #[test]
    fn enrichment_preserves_identity_order_and_count() {
        let root = project(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Alpha();\nnew PaymentGatewya();\n",
            ),
        ]);
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let outcome = analysis::analyze(&inputs).unwrap_or_default();
        let before = outcome.diagnostics.clone();
        let after = super::enrich(&session, &outcome.diagnostics);
        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), before.len(), "the count survives enrichment");
        assert!(
            before
                .iter()
                .zip(after.iter())
                .all(|(pre, post)| pre.id == post.id && pre.span() == post.span()),
            "the identity and order survive enrichment: {before:?} vs {after:?}",
        );
        // The property above would hold vacuously if nothing here
        // actually gained a suggestion: `PaymentGatewya` must.
        assert!(
            after
                .iter()
                .any(|diagnostic| !diagnostic.suggestions.is_empty()),
            "at least one diagnostic must actually be enriched: {after:?}",
        );
    }

    #[test]
    fn a_cross_namespace_near_declaration_never_gains_an_applicable_edit() {
        // The only near class lives in `App\Other`, not `App\Billing`:
        // rewriting the written terminal segment would still leave a
        // reference that does not resolve, so guard 2 must refuse it.
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App\\Other;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Billing\\PaymentGatewya();\n",
            ),
        ]);
        let class: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0018")
            .collect();
        assert_eq!(class.len(), 1);
        // The load-bearing property: no applicable edit is ever
        // produced across a namespace boundary. The qualified keys
        // `App\Billing\PaymentGatewya` and `App\Other\PaymentGateway`
        // differ enough (`Billing` vs `Other`) to also fall outside
        // the bounded distance, so this degrades all the way to
        // nothing, not even a note; see the task report.
        assert!(class[0].suggestions.is_empty());
        assert!(class[0].notes.is_empty());
    }

    #[test]
    fn a_namespace_typo_produces_a_note_naming_the_fully_qualified_name() {
        // `Biling\PaymentGateway` differs from the declared
        // `App\Billing\PaymentGateway` only in its namespace segment
        // (a missing `l`): guard 1 holds (both terminals are
        // `PaymentGateway`) but guard 2 fails (the qualifiers
        // `App\Biling` and `App\Billing` differ), so this degrades to
        // a note carrying the fully qualified declared name.
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App\\Billing;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Biling\\PaymentGateway();\n",
            ),
        ]);
        let class: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0018")
            .collect();
        assert_eq!(class.len(), 1);
        assert!(class[0].suggestions.is_empty());
        assert_eq!(
            class[0].notes,
            vec!["did you mean `App\\Billing\\PaymentGateway`?"]
        );
    }

    #[test]
    fn an_unknown_method_suggests_the_near_member_and_edits_exactly_its_token() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.id.as_str(), "CEL0030");
        assert_eq!(diagnostic.suggestions.len(), 1);
        let suggestion = &diagnostic.suggestions[0];
        assert_eq!(suggestion.message, "did you mean `save`?");
        assert_eq!(suggestion.confidence, Confidence::NeedsReview);
        // The edit covers exactly the member token, not the whole
        // member expression the diagnostic's span covers.
        let source = std::fs::read_to_string(_root.path().join("src/Caller.php")).unwrap();
        let edit = &suggestion.edits[0];
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        assert_eq!(&source[start..end], "svae");
        assert_eq!(edit.replacement, "save");
    }

    #[test]
    fn an_unknown_property_suggests_the_near_property() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public string $name = ''; }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction read(User $user): string { return $user->nmae; }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id.as_str(), "CEL0031");
        assert_eq!(
            diagnostics[0].suggestions[0].message,
            "did you mean `name`?",
        );
        assert_eq!(diagnostics[0].suggestions[0].edits[0].replacement, "name");
    }

    #[test]
    fn an_unknown_class_constant_and_enum_case_suggest_their_near_siblings() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Config.php",
                "<?php\nnamespace App;\nclass Config { public const LIMIT = 10; }\n",
            ),
            (
                "src/Status.php",
                "<?php\nnamespace App;\nenum Status { case Active; }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction f(): void { echo Config::LIMTI; $s = Status::Activ; }\n",
            ),
        ]);
        let messages: Vec<(&str, &str)> = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic
                    .suggestions
                    .first()
                    .map(|suggestion| (diagnostic.id.as_str(), suggestion.message.as_str()))
            })
            .collect();
        assert_eq!(
            messages,
            vec![
                ("CEL0032", "did you mean `LIMIT`?"),
                ("CEL0033", "did you mean `Active`?"),
            ],
        );
    }

    #[test]
    fn a_stub_inherited_member_is_a_candidate() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Caller.php",
                "<?php\nfunction f(\\ArrayObject $a): void { $a->getArrayCop(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id.as_str(), "CEL0030");
        assert_eq!(
            diagnostics[0].suggestions[0].message,
            "did you mean `getArrayCopy`?",
        );
    }

    #[test]
    fn a_member_with_no_near_sibling_stays_untouched() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction f(User $user): void { $user->frobnicate(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].suggestions.is_empty());
        assert!(diagnostics[0].notes.is_empty());
    }

    #[test]
    fn the_message_parser_matches_the_emitters_pinned_formats() {
        // These four literals are the exact formats
        // `crates/celerrate_rules/src/rules/unknown_members.rs` emits;
        // if that file changes shape, this test names the coupling.
        assert_eq!(
            super::parse_member_message("unknown method `svae` on `User`"),
            Some(("svae".to_owned(), "User".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown property `$nmae` on `App\\User`"),
            Some(("nmae".to_owned(), "App\\User".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown class constant `LIMTI` on `Config`"),
            Some(("LIMTI".to_owned(), "Config".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown enum case `Activ` on `Status`"),
            Some(("Activ".to_owned(), "Status".to_owned())),
        );
        assert_eq!(super::parse_member_message("no backticks here"), None);
    }

    #[test]
    fn the_member_token_is_the_operator_prefixed_occurrence() {
        use celerrate_source::TextSize;
        // `$svae->svae()`: the receiver spells the same word; the
        // token after `->` is the one the edit must cover.
        let range = super::member_token_range("$svae->svae()", "svae", TextSize::from(10)).unwrap();
        assert_eq!(u32::from(range.start()), 10 + 7);
        assert_eq!(u32::from(range.end()), 10 + 11);
        assert_eq!(
            super::member_token_range("Config::LIMTI", "LIMTI", TextSize::from(0))
                .map(|range| (u32::from(range.start()), u32::from(range.end())),),
            Some((8, 13)),
        );
        // No operator-prefixed occurrence: no range, never a guess.
        assert_eq!(
            super::member_token_range("svae", "svae", TextSize::from(0)),
            None,
        );
    }

    #[test]
    fn a_scoped_receiver_prefers_the_namespaced_class_over_a_same_named_global_one() {
        // Two classes share the bare name `Config`: one in the global
        // namespace (declared in a classmap root, not under `src/`), one
        // in `App`. The scoped access is written from inside `App`, so
        // PHP resolves the bare `Config` to `App\Config` with no global
        // fallback -- the near-miss suggestion must name `App\Config`'s
        // member (`LIMIT`), never the global class's (`CAP`). Pins the
        // escalation order in `receiver_class_key`: resolving the
        // written class-like reference through the file's own namespace
        // must be tried before ever trusting the bare fold, because the
        // bare fold alone would find the global `Config` first and
        // silently build the wrong candidate pool.
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST_WITH_LEGACY),
            (
                "legacy.php",
                "<?php\nclass Config { public const CAP = 1; }\n",
            ),
            (
                "src/Config.php",
                "<?php\nnamespace App;\nclass Config { public const LIMIT = 10; }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction f(): void { echo Config::LIMTI; }\n",
            ),
        ]);
        let class_constant: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0032")
            .collect();
        assert_eq!(class_constant.len(), 1);
        assert_eq!(class_constant[0].suggestions.len(), 1);
        assert_eq!(
            class_constant[0].suggestions[0].message, "did you mean `LIMIT`?",
            "the namespaced App\\Config::LIMIT must win, not the global Config::CAP",
        );
    }

    #[test]
    fn an_aliased_reference_never_gains_an_applicable_edit() {
        // Modeled on `celerrate_rules::rules::unknown_symbols`'s own
        // `use Lib\Missing as M; $x = new M();` fixture. `Lib\Mising`
        // is declared nearby (a real candidate exists in key space),
        // but the written terminal `M` shares nothing with the
        // resolved terminal `Missing`: guard 1 must refuse the edit.
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Declared.php",
                "<?php\nnamespace Lib;\nclass Mising {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nuse Lib\\Missing as M;\n$x = new M();\n",
            ),
        ]);
        let class: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0018")
            .collect();
        assert_eq!(class.len(), 1);
        assert!(class[0].suggestions.is_empty());
    }

    // `bounded_distance` and `did_you_mean` are test-only convenience
    // wrappers that themselves call `bounded_distance_pooled` and
    // `did_you_mean_pooled`: they are not independent implementations,
    // so comparing pooled output against them cannot catch a
    // regression in the pooled algorithm. What the next two tests
    // genuinely verify is scratch-reuse safety — one `DistanceScratch`
    // driven across many successive calls in a loop must produce the
    // same answers as a fresh scratch per call, which is exactly what
    // each wrapper constructs internally. The real behavior pin for
    // this refactor is the suite of fixture-driven enrichment tests
    // further down this module (starting with
    // `an_unknown_class_with_one_near_declaration_gains_an_applicable_suggestion`),
    // which run the pooled path end-to-end against real diagnostics.
    #[test]
    fn reusing_the_scratch_across_calls_does_not_leak_state_into_bounded_distance_pooled() {
        let mut scratch = super::DistanceScratch::default();
        let cases: [(&str, &str, usize); 6] = [
            ("svae", "save", 2),
            ("nmae", "name", 2),
            ("php_eol", "PHP_EOL", 2),
            ("draft", "active", 2),
            ("a", "abcd", 2),
            ("Activ", "Active", 2),
        ];
        for (written, candidate, bound) in cases {
            let written_lowercase: Vec<char> = written.to_lowercase().chars().collect();
            let candidate_lowercase: Vec<char> = candidate.to_lowercase().chars().collect();
            // The same scratch across every case, checked against
            // `bounded_distance`, which builds a fresh scratch per
            // call: reuse must not leak one computation's rows into
            // the next.
            assert_eq!(
                super::bounded_distance_pooled(
                    &written_lowercase,
                    &candidate_lowercase,
                    bound,
                    &mut scratch,
                ),
                super::bounded_distance(written, candidate, bound),
                "{written} vs {candidate}",
            );
        }
    }

    #[test]
    fn reusing_the_scratch_across_calls_does_not_leak_state_into_did_you_mean_pooled() {
        let mut scratch = super::DistanceScratch::default();
        let cases: [(&str, &[&str]); 4] = [
            ("svae", &["save", "wave", "unrelated"]),
            ("sive", &["sove", "save", "sove"]),
            ("svae", &["unrelated"]),
            ("Activ", &["Active", "Passive"]),
        ];
        for (written, candidates) in cases {
            let owned: Vec<String> = candidates.iter().map(|name| (*name).to_owned()).collect();
            let pool = super::pool_entries(owned.clone(), |name| name.to_owned());
            // The same scratch across every case, checked against
            // `did_you_mean`, which builds a fresh pool and a fresh
            // scratch per call.
            let (pooled, _) = super::did_you_mean_pooled(written, &pool, None, &mut scratch);
            assert_eq!(pooled, super::did_you_mean(written, owned), "{written}");
        }
    }

    #[test]
    fn a_fold_excluded_entry_never_becomes_a_candidate() {
        let mut scratch = super::DistanceScratch::default();
        let pool = super::pool_entries(vec!["save".to_owned(), "wave".to_owned()], |name| {
            format!("folded::{name}")
        });
        let (outcome, _) =
            super::did_you_mean_pooled("svae", &pool, Some("folded::save"), &mut scratch);
        // `save` is fold-excluded; `wave` is at distance 2, outside the
        // short-name bound of 1: nothing survives.
        assert_eq!(outcome, super::DidYouMean::Nothing);
    }
}
