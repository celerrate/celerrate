<?php
$result = [1, 2, 3]
    |> fn (array $list) => array_map(fn ($item) => $item ** 2, $list)
    |> array_filter(...);
$total = match (true) {
    $result === [] => throw new RuntimeException('empty'),
    default => array_sum($result),
};
echo "Total: {$total} for $user->name", PHP_EOL;
$copy = clone($order, ['id' => null, 'lines' => [...$order->lines]]);
$counter = static function (?int $seed = null) use (&$state): int {
    $state ??= $seed ?? 0;
    return $state++;
};
$label = $count > 1 ? "$count items" : ($count === 1 ? 'one item' : 'empty');
[$first, [, $third]] = $matrix[0];
$dispatcher?->events[static::class][] = fn () => yield from $queue;
