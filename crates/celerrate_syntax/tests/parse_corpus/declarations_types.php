<?php

function scalars(int $a, ?string $b, float|bool $c): void {}
function unions(int|string|null $x): int|false {}
function intersections(Countable&ArrayAccess $x): static {}
function dnf((Countable&ArrayAccess)|null $x): (Traversable&Countable)|false {}
function references(A&$x, B &...$rest): never {}
function relative(namespace\Kind $x, \Fully\Qualified $y): parent {}
