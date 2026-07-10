<?php
match ($status) {
    200, 204 => 'success',
    301, 302 => 'redirect',
    default => 'other',
};
$label = match (true) {
    $age >= 65 => 'senior',
    $age >= 18 => 'adult',
    default => 'minor',
};
match ($x) {};
$nested = match ($outer) {
    1 => match ($inner) {
        default => 'deep',
    },
    default => 'shallow',
};
