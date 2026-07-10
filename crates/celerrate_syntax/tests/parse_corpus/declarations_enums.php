<?php

enum Direction {
    case North;
    case South;
}

enum Suit: string implements HasColor {
    case Hearts = 'H';
    case Spades = 'S';

    const WILDCARD = '*';

    public function color(): string {
        return match ($this) {
            Suit::Hearts => 'red',
            Suit::Spades => 'black',
        };
    }
}

enum(1);
