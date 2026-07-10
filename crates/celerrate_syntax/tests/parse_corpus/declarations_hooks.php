<?php

class Person {
    public string $name {
        get => strtoupper($this->name);
        set(string $value) {
            $this->name = trim($value);
        }
    }

    public private(set) DateTimeImmutable $created;

    public array $items {
        final &get { return $this->items; }
    }

    public function __construct(
        public readonly string $first,
        private(set) string $last = 'unknown',
        public string $full { get => $this->first . ' ' . $this->last; },
    ) {}
}

interface Named {
    public string $name { get; set; }
}
