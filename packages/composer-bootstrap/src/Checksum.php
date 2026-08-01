<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/** SHA-256 verification against the release's SHA256SUMS body. */
final class Checksum
{
    /** Returns the hex hash recorded for the file, or null when absent. */
    public static function expectedFor(string $fileName, string $sums): ?string
    {
        foreach (preg_split('/\r?\n/', $sums) ?: [] as $line) {
            if (preg_match('/^([0-9a-f]{64})[ \t]+\*?(.+)$/', trim($line), $matches) === 1
                && $matches[2] === $fileName
            ) {
                return $matches[1];
            }
        }
        return null;
    }

    public static function matches(string $filePath, string $expectedHash): bool
    {
        $actual = hash_file('sha256', $filePath);
        return is_string($actual) && hash_equals($expectedHash, $actual);
    }
}
