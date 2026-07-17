//! The body IR: the range-free, `Eq`-comparable lowering of one
//! function or method body, behind a per-body salsa query. The arenas
//! hold expressions and statements densely numbered; no text offset
//! ever enters the IR, so an edit above a body, and a formatting-only
//! or ignorable-trivia edit inside it, produces an identical value
//! that salsa backdates: body consumers are structurally spared.
//! Spans reconcile late through the sibling source-map query, the
//! `ItemTree`/`AstIdMap` split one level down.
//!
//! Deferred, recorded: property-hook bodies (PHP 8.4) are not lowered
//! yet; they join when the corpus demands them.

use celerrate_db::SourceFile;
use celerrate_syntax::{SyntaxKind, SyntaxNodePtr};

use crate::ast_id::AstId;

/// The dense index of one expression in its body's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExpressionId(u32);

impl ExpressionId {
    /// A dangling index used when the arena would exceed `u32`:
    /// unreachable behind the 4 GiB source cap, total anyway.
    pub(crate) const OVERFLOW: Self = Self(u32::MAX);

    /// Constructs an id from a dense arena index, `None` past the
    /// `u32` cap (the `OVERFLOW` sentinel is never a valid id). Public
    /// so a check walker enumerating `BodyIr::expressions` can rebuild
    /// the id its own index corresponds to; never cast an index with
    /// `as`.
    pub fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index)
            .ok()
            .filter(|&value| value != u32::MAX)
            .map(Self)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

/// The dense index of one statement in its body's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StatementId(u32);

impl StatementId {
    /// See [`ExpressionId::OVERFLOW`].
    pub(crate) const OVERFLOW: Self = Self(u32::MAX);

    pub(crate) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index)
            .ok()
            .filter(|&value| value != u32::MAX)
            .map(Self)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

/// A member reference in an access expression: a named property, a
/// variable property, or a computed expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemberReference {
    Missing,
    Named { name: String },
    Variable { name: String },
    Computed { expression: ExpressionId },
}

/// A class reference in a `new` expression: named class, `self`,
/// `static`, a dynamic expression, or an anonymous class declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassReference {
    Missing,
    Named { name: String },
    StaticKeyword,
    Dynamic { expression: ExpressionId },
    Anonymous { declaration: AstId },
}

/// One argument in a call: optional label, optional spread, and value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallArgument {
    pub label: Option<String>,
    pub spread: bool,
    pub value: ExpressionId,
}

/// One capture in a closure's `use (...)` clause.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClosureUse {
    pub name: String,
    pub by_reference: bool,
}

/// One expression, lowered. `Option` fields encode valid absence (a
/// short ternary's middle); [`BodyExpression::Missing`] encodes
/// wreckage (a child error recovery could not produce).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BodyExpression {
    Missing,
    /// An integer, float, or single-quoted string, as written.
    /// (`true`, `null`, `self`, and the magic constants parse as
    /// names and lower to [`BodyExpression::NamedReference`].)
    Literal {
        text: String,
    },
    /// `$name`, the sigil stripped.
    Variable {
        name: String,
    },
    /// A name in expression position (a constant fetch, a callee, a
    /// class reference), as written; also the bare `static` keyword.
    NamedReference {
        text: String,
    },
    /// `$$name` and `${expression}`.
    DynamicVariable {
        target: ExpressionId,
    },
    /// Prefix operators: `-$a`, `!$a`, `~$a`, `&$a`, `@$a`, etc.
    Unary {
        operator: SyntaxKind,
        operand: ExpressionId,
    },
    /// Postfix operators: `$a++`, `$a--`.
    Postfix {
        operator: SyntaxKind,
        operand: ExpressionId,
    },
    /// Binary operators: `$a + $b`, `$a . $b`, `$a instanceof Foo`, `$a ?? $b`, etc.
    Binary {
        operator: SyntaxKind,
        lhs: ExpressionId,
        rhs: ExpressionId,
    },
    /// Assignment and compound assignment: `$a = $b`, `$a += $b`, `$a ??= $b`, etc.
    Assignment {
        operator: SyntaxKind,
        by_reference: bool,
        target: ExpressionId,
        value: ExpressionId,
    },
    /// Type cast: `(int) $a`, `(string) $b`, etc.
    Cast {
        operator: SyntaxKind,
        operand: ExpressionId,
    },
    /// Ternary: `$a ? $b : $c` or short ternary: `$a ?: $c`.
    Ternary {
        condition: ExpressionId,
        middle: Option<ExpressionId>,
        alternative: ExpressionId,
    },
    /// Array literal: `[$a, $b]` or `list($a, $b)`.
    Array {
        entries: Vec<ArrayEntry>,
    },
    /// Interpolated string: `"a {$x} b"` or heredoc `<<<EOT ... EOT`.
    InterpolatedString {
        parts: Vec<StringPart>,
    },
    /// Shell exec: `` `ls $dir` ``.
    ShellExec {
        parts: Vec<StringPart>,
    },
    /// `isset($a, $b)`.
    Isset {
        targets: Vec<ExpressionId>,
    },
    /// `empty($x)`.
    Empty {
        target: ExpressionId,
    },
    /// `eval($code)`.
    Eval {
        argument: ExpressionId,
    },
    /// `exit` or `die`.
    Exit {
        argument: Option<ExpressionId>,
    },
    /// `print $x`.
    Print {
        operand: ExpressionId,
    },
    /// `clone $x`.
    Clone {
        operand: ExpressionId,
    },
    /// `throw $x`.
    Throw {
        operand: ExpressionId,
    },
    /// `yield` or `yield $v` or `yield $k => $v` or `yield from $g`.
    Yield {
        key: Option<ExpressionId>,
        value: Option<ExpressionId>,
        delegated: bool,
    },
    /// `include`, `require`, `include_once`, `require_once`.
    Include {
        operator: SyntaxKind,
        operand: ExpressionId,
    },
    /// `match ($x) { 1, 2 => 'low', default => 'other' }`.
    Match {
        subject: ExpressionId,
        arms: Vec<MatchCase>,
    },
    /// `subject->name` and `subject?->name`; the `null_safe` flag distinguishes them.
    MemberAccess {
        receiver: ExpressionId,
        member: MemberReference,
        null_safe: bool,
    },
    /// `Foo::$prop` or `Foo::bar`.
    ScopedAccess {
        subject: ExpressionId,
        member: MemberReference,
    },
    /// Exactly-once wrapper of a dereference chain containing `?->`.
    NullSafeChain {
        chain: ExpressionId,
    },
    /// `foo(1, 2)` or `$obj->m(...$args)`.
    Call {
        callee: ExpressionId,
        arguments: Vec<CallArgument>,
    },
    /// First-class callable: `strlen(...)`.
    CallableReference {
        callee: ExpressionId,
    },
    /// `new Foo(1)`, `new self`, `new static`, `new $x`, or `new class { }`.
    New {
        class: ClassReference,
        arguments: Vec<CallArgument>,
    },
    /// `$a[0]` or `$a[]` (push).
    Index {
        subject: ExpressionId,
        index: Option<ExpressionId>,
    },
    /// `function (int $a) use ($x) { ... }` or `static function() { ... }`.
    Closure {
        parameters: Vec<crate::members::ParameterSignature>,
        uses: Vec<ClosureUse>,
        return_type_text: Option<String>,
        is_static: bool,
        by_reference: bool,
        body: Vec<StatementId>,
    },
    /// `fn (int $x): int => $x + 1` or `static fn() => 42`.
    ArrowFunction {
        parameters: Vec<crate::members::ParameterSignature>,
        return_type_text: Option<String>,
        is_static: bool,
        by_reference: bool,
        body: ExpressionId,
    },
}

/// One statement, lowered. Same absence rule as the expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BodyStatement {
    Missing,
    Block {
        statements: Vec<StatementId>,
    },
    Expression {
        expression: ExpressionId,
    },
    Return {
        value: Option<ExpressionId>,
    },
    Echo {
        values: Vec<ExpressionId>,
    },
    Break {
        level: Option<ExpressionId>,
    },
    Continue {
        level: Option<ExpressionId>,
    },
    Global {
        targets: Vec<ExpressionId>,
    },
    StaticVariables {
        variables: Vec<StaticVariableDeclaration>,
    },
    Unset {
        targets: Vec<ExpressionId>,
    },
    Goto {
        label: Option<String>,
    },
    Label {
        name: Option<String>,
    },
    If {
        condition: ExpressionId,
        then_branch: Vec<StatementId>,
        else_branch: Vec<StatementId>,
    },
    While {
        condition: ExpressionId,
        body: Vec<StatementId>,
    },
    DoWhile {
        body: Vec<StatementId>,
        condition: ExpressionId,
    },
    For {
        initializers: Vec<ExpressionId>,
        conditions: Vec<ExpressionId>,
        updates: Vec<ExpressionId>,
        body: Vec<StatementId>,
    },
    Foreach {
        subject: ExpressionId,
        key: Option<ExpressionId>,
        value: ExpressionId,
        by_reference: bool,
        body: Vec<StatementId>,
    },
    Switch {
        subject: ExpressionId,
        cases: Vec<SwitchArm>,
    },
    Try {
        body: Vec<StatementId>,
        catches: Vec<CatchArm>,
        finally: Option<Vec<StatementId>>,
    },
    Declare {
        statements: Vec<StatementId>,
    },
    Declaration {
        declaration: AstId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StaticVariableDeclaration {
    pub name: String,
    pub initializer: Option<ExpressionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SwitchArm {
    pub condition: Option<ExpressionId>,
    pub statements: Vec<StatementId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatchArm {
    pub types: Vec<String>,
    pub variable: Option<String>,
    pub statements: Vec<StatementId>,
}

/// One entry in an array literal or list destructuring.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayEntry {
    /// A destructuring hole: `[, $second]`.
    Hole,
    /// An element with optional key, possible spread, and optional by-reference.
    Element {
        key: Option<ExpressionId>,
        value: ExpressionId,
        spread: bool,
        by_reference: bool,
    },
}

/// One part of an interpolated string (including heredocs) or shell-exec.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StringPart {
    /// A text fragment between interpolations.
    Fragment { text: String },
    /// A simple interpolation (`$var`, `$obj->prop`, `$arr[0]`) with its written form.
    Simple { text: String },
    /// A complex interpolation: `{$expr}`, `${expr}`, or `{$obj->method()}`.
    Interpolation { expression: ExpressionId },
}

/// One arm of a match expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchCase {
    pub conditions: Vec<ExpressionId>,
    pub is_default: bool,
    pub body: ExpressionId,
}

/// One recognized annotation-bearing comment inside a body: content a
/// type-engine reader consumes, carried in the IR so an edit to it
/// invalidates body consumers while prose trivia stays invisible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BodyAnnotation {
    /// The comment text, verbatim.
    pub text: String,
    /// The first lowered statement starting after the comment ends;
    /// `None` when the comment trails every statement of the body.
    /// A trailing same-line directive (`@phpstan-ignore-line`) therefore
    /// anchors past its own line or to `None`; consumers matching that
    /// form reconcile through the source map instead.
    pub anchor: Option<StatementId>,
}

/// Whether one comment token is recognized annotation content: every
/// docblock (inline `@var`, assertion tags, anything a tag reader may
/// consume), plus line and block comments carrying a suppression
/// directive. The redefined comment-only edit class is exactly the
/// complement: trivia this predicate rejects never changes a body IR.
pub fn is_recognized_annotation(kind: SyntaxKind, text: &str) -> bool {
    match kind {
        SyntaxKind::DocComment => true,
        SyntaxKind::LineComment | SyntaxKind::BlockComment => {
            text.contains("@phpstan-ignore") || text.contains("@psalm-suppress")
        }
        _ => false,
    }
}

/// The lowered body of one function or method: dense arenas, the
/// top-level statement list, no text offset anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyIr {
    pub expressions: Vec<BodyExpression>,
    pub statements: Vec<BodyStatement>,
    /// The body's top-level statements, in source order.
    pub root: Vec<StatementId>,
    /// Recognized annotation-bearing comments, in document order.
    pub annotations: Vec<BodyAnnotation>,
}

impl BodyIr {
    pub fn expression(&self, id: ExpressionId) -> Option<&BodyExpression> {
        self.expressions.get(id.0 as usize)
    }

    pub fn statement(&self, id: StatementId) -> Option<&BodyStatement> {
        self.statements.get(id.0 as usize)
    }
}

/// Arena indices back to nodes: the range-carrying sibling of
/// [`BodyIr`], free to change on every edit, consulted only at
/// rendering time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodySourceMap {
    pub(crate) expressions: Vec<SyntaxNodePtr>,
    pub(crate) statements: Vec<SyntaxNodePtr>,
}

impl BodySourceMap {
    /// The pointer of one lowered expression. A `Missing` expression
    /// and a synthetic node (a null-safe chain wrapper) point at the
    /// nearest enclosing written node.
    pub fn expression_pointer(&self, id: ExpressionId) -> Option<SyntaxNodePtr> {
        self.expressions.get(id.0 as usize).copied()
    }

    pub fn statement_pointer(&self, id: StatementId) -> Option<SyntaxNodePtr> {
        self.statements.get(id.0 as usize).copied()
    }
}

/// One body to lower: the declaration `AstId` of a function or method.
#[salsa::interned(debug)]
pub struct BodyQuery<'db> {
    pub ast_id: AstId,
}

/// The body IR of one declaration: `None` when the identity does not
/// name a function or method carrying a body in `file` (an abstract or
/// interface method, a property, a mismatched file). Range-free, so an
/// ignorable edit backdates and body consumers are spared. No
/// artifact-cache consultation yet: the typed-artifact classes are
/// plan 9a.
#[salsa::tracked(returns(ref))]
pub fn body_ir<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodyIr> {
    lowered_body(db, file, body).map(|(ir, _)| ir)
}

/// The source map of one body: the range-carrying sibling of
/// [`body_ir`], re-running the same walk. The duplicate walk is the
/// price of the split; the cutoff it buys is the point.
#[salsa::tracked(returns(ref))]
pub fn body_source_map<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodySourceMap> {
    lowered_body(db, file, body).map(|(_, map)| map)
}

fn lowered_body<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<(BodyIr, BodySourceMap)> {
    let ast_id = body.ast_id(db);
    if ast_id.file != file.file_id(db) {
        return None;
    }
    let map = crate::queries::ast_id_map(db, file);
    let pointer = map.pointer(ast_id.index)?;
    let root = celerrate_db::parse(db, file).tree();
    let node = pointer.try_to_node(&root)?;
    crate::body_lowering::lower_body(ast_id.file, map, &node)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;
    use celerrate_syntax::SyntaxKind;

    use crate::ast_id::AstId;

    use super::{BodyQuery, body_ir, body_source_map};

    #[test]
    fn the_body_query_lowers_a_function_body() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let body = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        let ir = body_ir(&db, file, body).as_ref().unwrap();
        assert_eq!(ir.root.len(), 1);
    }

    #[test]
    fn a_method_is_addressed_by_its_member_index() {
        // Numbering: class = 0, method = 1 (the 1a contract).
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php class A { public function m() { return 1; } }".to_vec(),
        );
        let class = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        assert!(body_ir(&db, file, class).is_none());
        let method = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 1,
            },
        );
        assert!(body_ir(&db, file, method).is_some());
    }

    #[test]
    fn a_mismatched_file_or_unknown_index_answers_none() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let wrong_file = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(9),
                index: 0,
            },
        );
        assert!(body_ir(&db, file, wrong_file).is_none());
        let unknown = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 99,
            },
        );
        assert!(body_ir(&db, file, unknown).is_none());
    }

    #[test]
    fn the_source_map_query_reconciles_an_expression() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let body = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        let ir = body_ir(&db, file, body).as_ref().unwrap();
        let map = body_source_map(&db, file, body).as_ref().unwrap();

        let super::BodyStatement::Return { value: Some(value) } =
            ir.statement(*ir.root.first().unwrap()).unwrap()
        else {
            panic!("expected a return");
        };
        let pointer = map.expression_pointer(*value).unwrap();
        assert_eq!(pointer.kind(), SyntaxKind::Literal);
    }

    #[test]
    fn the_recognized_annotation_predicate_is_pinned() {
        use celerrate_syntax::SyntaxKind;

        use super::is_recognized_annotation;

        assert!(is_recognized_annotation(
            SyntaxKind::DocComment,
            "/** anything */"
        ));
        assert!(is_recognized_annotation(
            SyntaxKind::LineComment,
            "// @phpstan-ignore-line"
        ));
        assert!(is_recognized_annotation(
            SyntaxKind::LineComment,
            "# @psalm-suppress Foo"
        ));
        assert!(is_recognized_annotation(
            SyntaxKind::BlockComment,
            "/* @phpstan-ignore */"
        ));
        assert!(!is_recognized_annotation(
            SyntaxKind::LineComment,
            "// prose"
        ));
        assert!(!is_recognized_annotation(
            SyntaxKind::BlockComment,
            "/* prose */"
        ));
        assert!(!is_recognized_annotation(
            SyntaxKind::Whitespace,
            "@phpstan-ignore"
        ));
    }
}
