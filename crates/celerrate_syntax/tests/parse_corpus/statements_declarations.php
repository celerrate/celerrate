<?php
declare(strict_types=1);

function add(int $first, int $second = 0): int
{
    return $first + $second;
}

function &finder(callable $predicate): ?object
{
    static $cache = [], $hits = 0;
    global $registry;
    foreach ($registry as $entry) {
        if ($predicate($entry)) {
            $hits++;
            return $entry;
        }
    }
    unset($cache['stale']);
    goto missing;
    missing:
    return null;
}
