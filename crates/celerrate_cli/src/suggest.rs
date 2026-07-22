//! Presentation-time did-you-mean (design section 7): computed at
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
    ClassQuery, MemberKind, SymbolSpace, UseTables, collect_references, folded_member_key,
    folded_symbol_key, item_tree, linearized_class, resolve_candidates, source_symbol_table,
    stub_signature_table, stub_symbol_table,
};
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
use celerrate_stubs::StubMemberKind;

use crate::session::Session;

/// Optimal string alignment distance (restricted Damerau-Levenshtein)
/// over lowercased characters, abandoned as soon as it provably
/// exceeds `bound`. A transposition of two adjacent characters costs 1
/// edit, not 2: transposition is the dominant typo class (`svae` for
/// `save`, `nmae` for `name`) and plain Levenshtein overcharges it,
/// pushing exactly the typos this feature exists for outside the
/// bound. Lowercasing makes a case-only typo distance 0, which is
/// exactly the fix the case-sensitive spaces (constants, properties,
/// enum cases) want suggested.
fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize> {
    let written: Vec<char> = written.to_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    if written.len().abs_diff(candidate.len()) > bound {
        return None;
    }
    let mut before_previous: Vec<usize> = (0..=candidate.len()).collect();
    let mut previous: Vec<usize> = (0..=candidate.len()).collect();
    for (row, written_character) in written.iter().enumerate() {
        let mut current: Vec<usize> = Vec::with_capacity(candidate.len() + 1);
        current.push(row + 1);
        for (column, candidate_character) in candidate.iter().enumerate() {
            // The `get` fallbacks are unreachable (the rows are dense
            // by construction); they exist because indexing is denied
            // and a wrong answer here is caught by the tests anyway.
            let substitution = previous.get(column).copied().unwrap_or(usize::MAX - 1)
                + usize::from(written_character != candidate_character);
            let insertion = current.get(column).copied().unwrap_or(usize::MAX - 1) + 1;
            let deletion = previous.get(column + 1).copied().unwrap_or(usize::MAX - 1) + 1;
            let mut best = substitution.min(insertion).min(deletion);
            if row > 0 && column > 0 {
                let previous_written = written.get(row - 1);
                let previous_candidate = candidate.get(column - 1);
                if previous_written == Some(candidate_character)
                    && previous_candidate == Some(written_character)
                {
                    // Adjacent transposition: `..ab` -> `..ba` costs 1,
                    // read off the diagonal two rows up (the `get`
                    // fallback is unreachable for the same reason as
                    // above: the row two back is dense by construction
                    // whenever `row > 0`).
                    let transposition = before_previous
                        .get(column - 1)
                        .copied()
                        .unwrap_or(usize::MAX - 1)
                        + 1;
                    best = best.min(transposition);
                }
            }
            current.push(best);
        }
        if current.iter().min().copied().unwrap_or(0) > bound {
            return None;
        }
        before_previous = previous;
        previous = current;
    }
    previous
        .last()
        .copied()
        .filter(|&distance| distance <= bound)
}

/// The bound the design calls "bounded edit distance": tight for short
/// names (almost anything is within 2 of a 3-letter name), 2 otherwise.
fn distance_bound(name: &str) -> usize {
    if name.chars().count() <= 4 { 1 } else { 2 }
}

/// The ambiguity discipline (design section 7): a unique
/// minimal-distance candidate becomes an applicable suggestion; a tie
/// is listed in a note instead, because bulk `--fix-suggestions` must
/// never apply a guess the engine itself knows is ambiguous.
#[derive(Debug, PartialEq, Eq)]
enum DidYouMean {
    Nothing,
    Unique(String),
    Tie(Vec<String>),
}

fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean {
    let bound = distance_bound(written);
    let mut minimum: Option<usize> = None;
    let mut names: Vec<String> = Vec::new();
    for candidate in candidates {
        let Some(distance) = bounded_distance(written, &candidate, bound) else {
            continue;
        };
        match minimum {
            Some(best) if distance > best => {}
            Some(best) if distance == best => {
                if !names.contains(&candidate) {
                    names.push(candidate);
                }
            }
            _ => {
                minimum = Some(distance);
                names = vec![candidate];
            }
        }
    }
    names.sort();
    match names.len() {
        0 => DidYouMean::Nothing,
        1 => names.pop().map_or(DidYouMean::Nothing, DidYouMean::Unique),
        _ => DidYouMean::Tie(names),
    }
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
    classes: Option<Vec<String>>,
    functions: Option<Vec<String>>,
    constants: Option<Vec<String>>,
    /// The receiver's member names of one kind, keyed by (resolved
    /// class-like key, kind), built at most once per [`enrich`] call.
    /// Excludes nothing yet: the written member's own key is filtered
    /// out per diagnostic afterwards, since two diagnostics can share a
    /// receiver while writing different unknown members.
    members: HashMap<(String, MemberKind), Vec<String>>,
}

impl<'a> CandidatePools<'a> {
    fn new(session: &'a Session) -> Self {
        Self {
            session,
            classes: None,
            functions: None,
            constants: None,
            members: HashMap::new(),
        }
    }

    /// The declared qualified names of `space`, computed on first use
    /// and shared with every later call in the same pass.
    fn get(&mut self, space: SymbolSpace) -> &[String] {
        let session = self.session;
        let slot = match space {
            SymbolSpace::ClassLike => &mut self.classes,
            SymbolSpace::Function => &mut self.functions,
            SymbolSpace::Constant => &mut self.constants,
        };
        slot.get_or_insert_with(|| declared_pool(session, space))
    }

    /// The declared member names of `kind` on the class-like named by
    /// `class_key`, computed on first use per (class_key, kind) pair
    /// and shared with every later call in the same pass. `class_key`
    /// must already be the resolved fully qualified key (see
    /// `receiver_class_key`), not the as-written receiver text.
    fn member_pool(&mut self, class_key: &str, kind: MemberKind) -> &[String] {
        let session = self.session;
        self.members
            .entry((class_key.to_owned(), kind))
            .or_insert_with(|| member_candidates(session, class_key, kind))
    }
}

/// Every declared qualified name of `space`, source and stub halves
/// alike. Unlike the old terminal-segment pool, the qualified name is
/// kept whole: comparing keys rather than bare terminal segments is
/// the whole point of this design (see the module's task-2 report).
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
/// `celerrate_semantics::reference_checks`.
fn attempted_keys(
    session: &Session,
    file: FileId,
    range: TextRange,
    space: SymbolSpace,
) -> Option<Vec<String>> {
    let source = *session.sources.get(&file)?;
    let root = parse(&session.database, source).tree();
    let reference = collect_references(&root)
        .into_iter()
        .find(|reference| reference.range == range && reference.space == space)?;
    let tree = item_tree(&session.database, source);
    let tables = UseTables::for_namespace(tree, &reference.namespace);
    Some(resolve_candidates(
        &reference.written,
        space,
        &reference.namespace,
        &tables,
    ))
}

/// Runs `did_you_mean` once per attempted key (PHP tries more than one
/// only for the function/constant global fallback), against the pool
/// with that key's own fold-equal entries excluded (a name folding
/// equal to an attempted key would have resolved, so excluding it is
/// the per-diagnostic part of an otherwise shared pool). Returns the
/// attempted key with the nearest outcome and that outcome; on an
/// exact tie between two attempted keys the first in resolution order
/// wins, which is PHP's own precedence.
fn did_you_mean_across_keys(
    attempted: Vec<String>,
    pool: &[String],
    space: SymbolSpace,
) -> Option<(String, DidYouMean)> {
    let mut best: Option<(String, DidYouMean, usize)> = None;
    for key in attempted {
        let folded_key = folded_symbol_key(space, &key);
        let filtered: Vec<String> = pool
            .iter()
            .filter(|candidate| folded_symbol_key(space, candidate) != folded_key)
            .cloned()
            .collect();
        let bound = distance_bound(&key);
        let outcome = did_you_mean(&key, filtered);
        let distance = match &outcome {
            DidYouMean::Nothing => None,
            DidYouMean::Unique(candidate) => bounded_distance(&key, candidate, bound),
            // Every name in a tie shares the same minimal distance;
            // any of them reports it.
            DidYouMean::Tie(names) => names
                .first()
                .and_then(|candidate| bounded_distance(&key, candidate, bound)),
        };
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
    let attempted = attempted_keys(session, file, range, space)?;
    let pool = pools.get(space);
    let (winning_key, outcome) = did_you_mean_across_keys(attempted, pool, space)?;
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
    // anonymous class) yields no candidates and therefore no noise.
    if receiver.contains('|') || receiver.contains('@') {
        return None;
    }
    let class_key = receiver_class_key(session, file, range, &receiver);
    let written_key = folded_member_key(kind, &member);
    let pool = pools.member_pool(&class_key, kind);
    let candidates: Vec<String> = pool
        .iter()
        .filter(|candidate| folded_member_key(kind, candidate) != written_key)
        .cloned()
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let text = span_text(session, file, range)?;
    match did_you_mean(&member, candidates) {
        DidYouMean::Nothing => None,
        DidYouMean::Unique(candidate) => match member_token_range(&text, &member, range.start()) {
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
        },
        DidYouMean::Tie(names) => Some(Enrichment::Note(tie_note(&names))),
    }
}

/// The receiver's fully qualified class-like key. Instance access
/// (`$value->member`) reports the resolved key already
/// (`receiver_display` in `celerrate_types::checks::receivers`), so
/// folding it is enough and no syntax tree needs touching. A scoped
/// access (`Foo::CONST`, `Foo::method()`) instead reports the
/// as-written subject text verbatim (`written_class_display`, kept for
/// message legibility): when the folded written text names no known
/// class, this recomputes the true key from the file's own class-like
/// reference at this span, exactly like `attempted_keys` does for the
/// unknown-symbol families. Falling back to the folded written text
/// when even that fails is safe: an unresolvable key simply yields no
/// candidates below, which is the same "no enrichment" outcome as
/// returning `None` here would have produced.
fn receiver_class_key(session: &Session, file: FileId, range: TextRange, receiver: &str) -> String {
    let folded = folded_symbol_key(SymbolSpace::ClassLike, receiver);
    if class_exists(session, &folded) {
        return folded;
    }
    resolved_receiver_key(session, file, range, receiver).unwrap_or(folded)
}

/// Whether a fully qualified key names a known class-like, source or
/// stub — the same two lookups `member_candidates` itself walks from,
/// consulted here only to verify a resolution attempt.
fn class_exists(session: &Session, class_key: &str) -> bool {
    let db = &session.database;
    let class = ClassQuery::new(db, class_key.to_owned());
    if linearized_class(
        db,
        session.files,
        session.stubs,
        session.configuration,
        class,
    )
    .as_ref()
    .is_some()
    {
        return true;
    }
    stub_signature_table(db, session.stubs)
        .class(class_key)
        .is_some()
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
    file: FileId,
    range: TextRange,
    receiver: &str,
) -> Option<String> {
    let source = *session.sources.get(&file)?;
    let root = parse(&session.database, source).tree();
    let reference = collect_references(&root).into_iter().find(|reference| {
        reference.space == SymbolSpace::ClassLike
            && reference.written == receiver
            && range.contains_range(reference.range)
    })?;
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
/// the pool in this sub-project. `class_key` must already be resolved
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
    /// the single-pass path will (task 4).
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
}
