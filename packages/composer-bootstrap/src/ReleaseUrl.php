<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/**
 * Resolves where the artifacts download from. The binary version is
 * locked 1:1 to the package version: the base URL always names the
 * release tag matching the installed package.
 */
final class ReleaseUrl
{
    /**
     * The override wins when set (corporate mirrors, hermetic tests).
     * A development version has no release to download from: null, and
     * the caller reports it.
     */
    public static function baseUrl(string $packageVersion, ?string $override): ?string
    {
        if ($override !== null && $override !== '') {
            return rtrim($override, '/');
        }
        if (strpos($packageVersion, 'dev-') === 0 || substr($packageVersion, -4) === '-dev') {
            return null;
        }
        $tag = 'v' . ltrim($packageVersion, 'v');
        return "https://github.com/celerrate/celerrate/releases/download/{$tag}";
    }
}
