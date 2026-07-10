<?php

namespace App\Domain;

#[Attribute]
final readonly class Money
{
    public function __construct(
        private int $amount,
        private Currency $currency = Currency::Euro,
    ) {
    }

    public function add(self $other): static
    {
        return new static($this->amount + $other->amount, $this->currency);
    }
}

enum Currency: string
{
    case Euro = 'EUR';
    case Dollar = 'USD';
}
