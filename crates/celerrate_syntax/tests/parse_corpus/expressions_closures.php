<?php
$closure = function ($a, $b = 1) use (&$total, $rate) {
    echo $a;
};
$typed = function (int $x, ?\Foo\Bar $y = null, callable ...$rest): ?int {
    echo $x;
};
$byReference = function &(&$target) {
    echo $target;
};
$immediate = (function () { echo 'now'; })();
$arrow = fn ($x) => $x * 2;
$curried = static fn (int $x): callable => fn (int $y): int => $x + $y;
usort($items, fn ($a, $b) => $a->weight <=> $b->weight);
$mixed = function () { ?>chunk<?php echo 'back'; };
