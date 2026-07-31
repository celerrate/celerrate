<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Archive;
use PHPUnit\Framework\TestCase;

final class ArchiveTest extends TestCase
{
    private const TRIPLE = 'x86_64-unknown-linux-musl';

    private const WINDOWS_TRIPLE = 'x86_64-pc-windows-msvc';

    /** @var string[] */
    private $directoriesToClean = [];

    protected function tearDown(): void
    {
        foreach ($this->directoriesToClean as $directory) {
            self::removeDirectory($directory);
        }
        $this->directoriesToClean = [];
    }

    private function makeTemporaryDirectory(): string
    {
        $directory = sys_get_temp_dir() . '/celerrate-archive-test-' . bin2hex(random_bytes(8));
        mkdir($directory, 0755, true);
        $this->directoriesToClean[] = $directory;
        return $directory;
    }

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

    private function makeWindowsReleaseArchive(string $directory): string
    {
        $stage = $directory . '/stage/celerrate-' . self::WINDOWS_TRIPLE;
        mkdir($stage, 0755, true);
        file_put_contents($stage . '/celerrate.exe', "fake windows binary\n");
        file_put_contents($stage . '/LICENSE-MIT', "license\n");
        $zipPath = $directory . '/celerrate-' . self::WINDOWS_TRIPLE . '.zip';
        $archive = new \PharData($zipPath, 0, null, \Phar::ZIP);
        $archive->buildFromDirectory($directory . '/stage');
        return $zipPath;
    }

    public function testExtractsTheBinaryOutOfATarGzArchive(): void
    {
        $directory = $this->makeTemporaryDirectory();
        $archivePath = $this->makeReleaseArchive($directory);
        $binary = Archive::extractBinary($archivePath, self::TRIPLE, $directory . '/out');
        self::assertFileExists($binary);
        self::assertStringEndsWith('celerrate-' . self::TRIPLE . '/celerrate', $binary);
        self::assertSame("#!/bin/sh\necho fake\n", file_get_contents($binary));
    }

    public function testExtractsTheBinaryOutOfAZipArchive(): void
    {
        $directory = $this->makeTemporaryDirectory();
        $archivePath = $this->makeWindowsReleaseArchive($directory);
        $binary = Archive::extractBinary($archivePath, self::WINDOWS_TRIPLE, $directory . '/out');
        self::assertFileExists($binary);
        self::assertStringEndsWith('celerrate-' . self::WINDOWS_TRIPLE . '/celerrate.exe', $binary);
        self::assertSame("fake windows binary\n", file_get_contents($binary));
    }

    public function testRefusesAnArchiveWithoutTheExpectedBinary(): void
    {
        $directory = $this->makeTemporaryDirectory();
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

    private static function removeDirectory(string $directory): void
    {
        if (!is_dir($directory)) {
            return;
        }
        $entries = new \RecursiveIteratorIterator(
            new \RecursiveDirectoryIterator($directory, \FilesystemIterator::SKIP_DOTS),
            \RecursiveIteratorIterator::CHILD_FIRST
        );
        foreach ($entries as $entry) {
            $entry->isDir() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
        }
        rmdir($directory);
    }
}
