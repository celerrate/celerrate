<?php

class Account {
    var $legacy;
    public int $balance = 0, $pending = 0;
    protected static ?self $instance = null;
    final protected const int CEILING = 10;
    const FOR = 'semi-reserved';

    public function __construct() {}
    abstract public function close(): void;
    public function list(): array { return []; }
    public function &reference(): int { return $this->balance; }

    use Greets, Counts {
        Greets::hello insteadof Counts;
        Counts::hello as protected countedHello;
        list as unreserved;
        rename as private;
    }
}
