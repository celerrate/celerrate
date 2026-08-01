<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Platform;
use PHPUnit\Framework\TestCase;

final class PlatformTest extends TestCase
{
    public function testMapsTheSupportedPlatformsToTheReleaseTriples(): void
    {
        self::assertSame('x86_64-unknown-linux-musl', Platform::targetTriple('Linux', 'x86_64'));
        self::assertSame('aarch64-unknown-linux-musl', Platform::targetTriple('Linux', 'aarch64'));
        self::assertSame('x86_64-apple-darwin', Platform::targetTriple('Darwin', 'x86_64'));
        self::assertSame('aarch64-apple-darwin', Platform::targetTriple('Darwin', 'arm64'));
        self::assertSame('x86_64-pc-windows-msvc', Platform::targetTriple('Windows', 'AMD64'));
    }

    public function testReturnsNullForUnsupportedPlatforms(): void
    {
        self::assertNull(Platform::targetTriple('BSD', 'x86_64'));
        self::assertNull(Platform::targetTriple('Linux', 'riscv64'));
        self::assertNull(Platform::targetTriple('Windows', 'arm64'));
    }

    public function testArchiveAndBinaryNamesFollowTheTargetFamily(): void
    {
        self::assertSame(
            'celerrate-x86_64-unknown-linux-musl.tar.gz',
            Platform::archiveFileName('x86_64-unknown-linux-musl')
        );
        self::assertSame(
            'celerrate-x86_64-pc-windows-msvc.zip',
            Platform::archiveFileName('x86_64-pc-windows-msvc')
        );
        self::assertSame('celerrate', Platform::binaryFileName('aarch64-apple-darwin'));
        self::assertSame('celerrate.exe', Platform::binaryFileName('x86_64-pc-windows-msvc'));
    }
}
