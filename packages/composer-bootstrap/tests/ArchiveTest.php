<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Archive;
use PHPUnit\Framework\TestCase;

final class ArchiveTest extends TestCase
{
    private const TRIPLE = 'x86_64-unknown-linux-musl';

    private function makeReleaseArchive(string $directory): string
    {
        $stage = $directory . '/stage/celerrate-' . self::TRIPLE;
        mkdir($stage, 0755, true);
        file_put_contents($stage . '/celerrate', "#!/bin/sh\necho fake\n");
        file_put_contents($stage . '/LICENSE-MIT', "license\n");
        $tarPath = $directory . '/celerrate-' . self::TRIPLE . '.tar';
        $archive = new \PharData($tarPath);
        $archive->buildFromDirectory($directory . '/stage');
        $archive->compress(\Phar::GZ);
        unlink($tarPath);
        return $tarPath . '.gz';
    }

    public function testExtractsTheBinaryOutOfATarGzArchive(): void
    {
        $directory = sys_get_temp_dir() . '/celerrate-archive-test-' . bin2hex(random_bytes(8));
        mkdir($directory, 0755, true);
        $archivePath = $this->makeReleaseArchive($directory);
        $binary = Archive::extractBinary($archivePath, self::TRIPLE, $directory . '/out');
        self::assertFileExists($binary);
        self::assertStringEndsWith('celerrate-' . self::TRIPLE . '/celerrate', $binary);
        self::assertSame("#!/bin/sh\necho fake\n", file_get_contents($binary));
    }

    public function testRefusesAnArchiveWithoutTheExpectedBinary(): void
    {
        $directory = sys_get_temp_dir() . '/celerrate-archive-test-' . bin2hex(random_bytes(8));
        mkdir($directory . '/stage/unexpected', 0755, true);
        file_put_contents($directory . '/stage/unexpected/file', "body\n");
        $tarPath = $directory . '/celerrate-' . self::TRIPLE . '.tar';
        $archive = new \PharData($tarPath);
        $archive->buildFromDirectory($directory . '/stage');
        $archive->compress(\Phar::GZ);
        unlink($tarPath);
        $this->expectException(\RuntimeException::class);
        Archive::extractBinary($tarPath . '.gz', self::TRIPLE, $directory . '/out');
    }
}
