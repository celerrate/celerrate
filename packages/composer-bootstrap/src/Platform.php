<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/**
 * Maps the running platform onto the release target triple and the
 * artifact names the release publishes for it.
 */
final class Platform
{
    /** Returns the target triple, or null when the platform is unsupported. */
    public static function targetTriple(string $operatingSystemFamily, string $machine): ?string
    {
        $machine = strtolower($machine);
        $isX64 = in_array($machine, ['x86_64', 'amd64'], true);
        $isArm64 = in_array($machine, ['aarch64', 'arm64'], true);
        switch ($operatingSystemFamily) {
            case 'Linux':
                if ($isX64) {
                    return 'x86_64-unknown-linux-musl';
                }
                return $isArm64 ? 'aarch64-unknown-linux-musl' : null;
            case 'Darwin':
                if ($isX64) {
                    return 'x86_64-apple-darwin';
                }
                return $isArm64 ? 'aarch64-apple-darwin' : null;
            case 'Windows':
                return $isX64 ? 'x86_64-pc-windows-msvc' : null;
            default:
                return null;
        }
    }

    public static function archiveFileName(string $targetTriple): string
    {
        $extension = strpos($targetTriple, 'windows') !== false ? 'zip' : 'tar.gz';
        return "celerrate-{$targetTriple}.{$extension}";
    }

    public static function binaryFileName(string $targetTriple): string
    {
        return strpos($targetTriple, 'windows') !== false ? 'celerrate.exe' : 'celerrate';
    }
}
