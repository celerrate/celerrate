//! Pages for the lexer and parser error-resilience family
//! (CEL0002 to CEL0017): every way `celerrate_syntax` recovers from
//! malformed source while keeping the tree lossless and analysis
//! running past the recovered region.

use crate::explain::ExplainPage;

/// Every syntax page shares this configuration note: none of these
/// findings come from a rule, so none of them can be disabled.
const RESILIENCE: &str = "\
Produced by the parser's error resilience in `celerrate_syntax`, not
by a rule: it cannot be disabled, and analysis continues past the
recovered region.";

pub(crate) const CEL0002: ExplainPage = ExplainPage {
    why: "\
The lexer reached a byte that no lexing rule accepts in scripting
mode, for example a stray control character. No token can be built
from it, so it becomes a single-character `Error` token and lexing
resumes immediately after it: the rest of the file is still lexed and
parsed, with the offending byte reported at its own position.",
    failing_example: "<?php $x = 1 \u{1} 2;",
    fixed_example: "<?php $x = 1 + 2;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0003: ExplainPage = ExplainPage {
    why: "\
The lexer entered a block comment at `/*` and expected a matching
`*/` before the end of the file. None ever came, so the comment token
is recovered by running it all the way to the end of input: nothing
after the unterminated `/*` is available to lex or parse.",
    failing_example: "<?php /* never closed",
    fixed_example: "<?php /* closed */ $x = 1;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0004: ExplainPage = ExplainPage {
    why: "\
The lexer entered a quoted or backtick string and expected a matching
closing quote before the end of the file. None ever came, so the
string token is recovered by running it to the end of input, and the
parser then reports its own missing statement terminator on top of
the unterminated literal.",
    failing_example: "<?php $s = 'never closed;",
    fixed_example: "<?php $s = 'closed';",
    configuration: RESILIENCE,
};

pub(crate) const CEL0005: ExplainPage = ExplainPage {
    why: "\
The lexer entered a heredoc or nowdoc after `<<<TEXT` and expected the
closing label `TEXT` on a line by itself before the end of the file.
None ever came, so the heredoc body is recovered by running it to the
end of input, exactly like an unterminated quoted string but for the
label-delimited form.",
    failing_example: "<?php $s = <<<TEXT\nnever closed\n",
    fixed_example: "<?php $s = <<<TEXT\nclosed\nTEXT;\n",
    configuration: RESILIENCE,
};

pub(crate) const CEL0006: ExplainPage = ExplainPage {
    why: "\
Inside a double-quoted or heredoc string, `{$` opens complex
interpolation and the lexer expects a matching `}` before the string
itself closes. None ever came, so the interpolation is recovered by
treating the rest of the string as unterminated too: the enclosing
string's own unterminated-string finding fires alongside this one.",
    failing_example: "<?php $s = \"a {$x\";",
    fixed_example: "<?php $x = 1; $s = \"{$x}\";",
    configuration: RESILIENCE,
};

pub(crate) const CEL0007: ExplainPage = ExplainPage {
    why: "\
The grammar reached a position that requires an expression and found
a token that cannot start one, for example the right-hand side of an
assignment ending right at the statement terminator. The expression is
recovered as missing (absent from the tree) rather than guessed at,
and the surrounding statement still completes.",
    failing_example: "<?php $x = ;",
    fixed_example: "<?php $x = 1;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0008: ExplainPage = ExplainPage {
    why: "\
A statement expects its terminating `;` (or, where PHP itself allows
the omission, a closing `?>` or the end of input) and the next token
is neither: a second statement starts right where the terminator
belongs. The missing `;` is recovered as a zero-width gap at that
position, and the next statement still parses as its own statement.",
    failing_example: "<?php $a = 1 $b = 2;",
    fixed_example: "<?php $a = 1; $b = 2;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0009: ExplainPage = ExplainPage {
    why: "\
The grammar expects one specific token at this position and the
source holds something else: here, the `abstract` modifier expects
the `class` keyword to follow it and instead meets an expression
statement. The missing token is recovered as a zero-width gap at the
spot where it belongs, and parsing continues from there.",
    failing_example: "<?php abstract 1;",
    fixed_example: "<?php abstract class C {}",
    configuration: RESILIENCE,
};

pub(crate) const CEL0010: ExplainPage = ExplainPage {
    why: "\
A token appears at a position no grammar rule accepts, for example a
stray closing brace once its matching class body has already closed.
No rule can consume it as part of any construct, so it is wrapped in
its own `ErrorNode` and skipped, and the tokens around it keep parsing
normally.",
    failing_example: "<?php class C {} }",
    fixed_example: "<?php class C {}",
    configuration: RESILIENCE,
};

pub(crate) const CEL0011: ExplainPage = ExplainPage {
    why: "\
Every recursive descent into a sub-expression is metered against a
budget of 128 nested frames (`Parser::MAXIMUM_NESTING_DEPTH` in
`celerrate_syntax/src/parser.rs`): degenerate input like a long run of
`(` cannot be allowed to recurse the parser itself into a stack
overflow. Once the budget is exhausted, the guard refuses to descend
any further, diagnoses the refusal, and leaves the innermost
expression missing from the tree while every token is still preserved
through recovery. In the example below, the surrounding `$x = ` and
its own statement wrapper already claim two of those 128 frames before
the first `(` is even reached, so 126 nested parentheses are enough to
exhaust the remaining budget and trip the guard on the 129th attempted
descent; the fixed example is the same shape one level shallower,
comfortably inside the cap.",
    failing_example: "<?php $x = ((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))));",
    fixed_example: "<?php $x = (((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))));",
    configuration: RESILIENCE,
};

pub(crate) const CEL0012: ExplainPage = ExplainPage {
    why: "\
`<`, `>`, `<=`, `>=`, and a handful of other operators are
non-associative in PHP: Zend rejects chaining two of them at the same
precedence level without explicit parentheses, because the natural
left-to-right reading (`1 < 2 < 3` as `(1 < 2) < 3`) hides a type
coercion most authors do not intend. The chain is still parsed
left-associatively so the tree stays complete, but the chaining itself
is diagnosed.",
    failing_example: "<?php $x = 1 < 2 < 3;",
    fixed_example: "<?php $x = 1 < 2 && 2 < 3;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0013: ExplainPage = ExplainPage {
    why: "\
Every grammar loop is metered by a step budget: if an iteration
observes the current token without ever consuming it, the fuse blows
and the loop stops rather than spinning forever. This is a defensive
backstop against a parser bug, not something well-formed or even
malformed PHP source can trigger through the grammar itself; reaching
it means some grammar rule stopped making progress while tokens
remained. Once the fuse has blown, a single lossless recovery pass
sweeps every token the grammar never got to consume into one
`ErrorNode` at the end of the tree, so the source text is never lost
even when a loop misbehaves.",
    failing_example: "\
This page's example is narrative, not executed by the explain-page
harness: no grammar-admitted source reaches this backstop, only a
grammar loop that stopped consuming tokens by mistake. The crate's own
regression test drives the condition directly, without going through
any source text at all: it forces the parser's internal step counter
past its budget by repeatedly observing the current token without
bumping past it, then calls the same recovery pass `run` calls once
parsing finishes, and asserts that the unconsumed tail is swept into a
single `ErrorNode` with exactly one `NoProgress` diagnostic
(`crates/celerrate_syntax/src/parser.rs`,
`the_fuse_blows_after_the_step_budget_and_the_backstop_recovers_losslessly`).",
    fixed_example: "\
There is no source-level fix, because there is no source-level
trigger: this diagnostic reports a grammar rule that failed to make
progress, which is a parser defect to fix in `celerrate_syntax`
itself, not a pattern to rewrite in PHP source.",
    configuration: RESILIENCE,
};

pub(crate) const CEL0014: ExplainPage = ExplainPage {
    why: "\
`->`, `?->`, and `::` all expect a member name (a property, method, or
class constant) right after them, and here the source ends with
nothing usable following the arrow. The member name is recovered as
missing rather than guessed at, and the enclosing expression still
completes so the statement can be swept into the tree.",
    failing_example: "<?php $u->;",
    fixed_example: "<?php class C { public $prop = 1; } $u = new C(); $u->prop;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0015: ExplainPage = ExplainPage {
    why: "\
A control-flow body position (the single embedded statement of
`if ($x) body`) expects a statement, and here it sits directly against
the `else` keyword that closes the `if`: no body was ever written. The
embedded statement is diagnosed and recovered as missing without
consuming the `else`, so the `if` still claims its `else` clause and
recovery stays local to the one missing body.",
    failing_example: "<?php if ($x) else echo 1;",
    fixed_example: "<?php $x = true; if ($x) echo 0; else echo 1;",
    configuration: RESILIENCE,
};

pub(crate) const CEL0016: ExplainPage = ExplainPage {
    why: "\
A return-type position (after a function's `:`) expects a type, and
here the `:` is followed directly by the function body's opening
brace, with no type written between them. The type is recovered as
missing rather than guessed at, and the function's body still parses
normally.",
    failing_example: "<?php function f(): { }",
    fixed_example: "<?php function f(): void { }",
    configuration: RESILIENCE,
};

pub(crate) const CEL0017: ExplainPage = ExplainPage {
    why: "\
An attribute group (`#[...]`) always attaches to the declaration that
follows it, and here it is followed by an `echo` statement instead of
any declaration at all. The attribute group is wrapped into its own
`ErrorNode` rather than attached to something it cannot modify, and
the statement that follows it still parses as its own statement.",
    failing_example: "\
<?php
#[Attribute]
class Marker {}

#[Marker] echo 1;",
    fixed_example: "\
<?php
#[Attribute]
class Marker {}

#[Marker] class C {}",
    configuration: RESILIENCE,
};
