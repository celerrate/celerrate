<?php

#[Attribute(Attribute::TARGET_CLASS)]
class Route {}

#[Route('/home', methods: ['GET'])]
#[Deprecated]
final class HomeController {
    #[Override]
    public function handle(#[SensitiveParameter] string $token): void {}

    #[Marker]
    const MAPPED = 1;

    #[Marker]
    public int $count = 0;
}

enum Level {
    #[Description('lowest')]
    case Low;
}

$handler = #[Pure] static fn (int $x): int => $x * 2;
$instance = new #[Marker] class {};
