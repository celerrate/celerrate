<?php

namespace App;

use App\Contracts\{Countable as Sized, function assert_positive};

#[Entity(table: 'accounts')]
final class Account extends Base implements Sized, \Stringable
{
    use Auditable, Timestamps {
        Auditable::record insteadof Timestamps;
        Timestamps::record as protected recordTime;
    }

    public private(set) int $balance = 0 {
        get => $this->balance;
        set(int $value) { $this->balance = max(0, $value); }
    }

    final public const (Countable&Traversable)|null REGISTRY = null;

    public function __construct(
        #[Id] public readonly string $identifier,
        protected ?self $parent = null,
    ) {}

    abstract protected function audit(): void;

    public function count(): int { return $this->balance; }
}

enum Currency: string
{
    case Euro = 'EUR';

    public function symbol(): string
    {
        return match ($this) { Currency::Euro => '€' };
    }
}
