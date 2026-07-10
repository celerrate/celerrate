<?php

$mix = 0xFF_EC + 0b1010 - 0o777 + 0777 + 1_000_000;
$floats = .5 + 1. + 1.5e-3 + 2E8;
$compare = $a <=> $b ?: $a ?? $b;
$assign ??= $a ** $b % $c;
$casts = (int) '1' . (string)2 . ( FLOAT )$x;
$arrow = fn(int $n): int => $n <<= 2;
$attribute = new #[Pure] class {};
list($x, [$y]) = [1, [2]];
