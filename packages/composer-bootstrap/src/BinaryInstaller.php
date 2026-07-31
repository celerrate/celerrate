<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

use Composer\Composer;
use Composer\IO\IOInterface;
use Composer\Util\HttpDownloader;

/**
 * Downloads the release binary matching the installed package version,
 * verifies its SHA-256 against the release's SHA256SUMS, and places it
 * in <package>/bin-cache/ where the shim finds it.
 *
 * Failure stance: every failure is loud and actionable (a checksum
 * mismatch or a missing SHA256SUMS entry aborts), with one tolerance:
 * an unsupported platform warns and skips, because the bootstrap
 * package must never make the host project uninstallable. The shim
 * carries the error if celerrate is actually invoked.
 */
final class BinaryInstaller
{
    public function install(Composer $composer, IOInterface $io): void
    {
        $external = getenv('CELERRATE_BINARY');
        if (is_string($external) && $external !== '') {
            $io->write("celerrate: using the external binary at {$external} (CELERRATE_BINARY)");
            return;
        }
        $triple = Platform::targetTriple(PHP_OS_FAMILY, php_uname('m'));
        if ($triple === null) {
            $io->writeError(
                '<warning>celerrate: unsupported platform (' . PHP_OS_FAMILY . '/' . php_uname('m')
                . '); the binary was not installed. See https://github.com/celerrate/celerrate for the other install channels.</warning>'
            );
            return;
        }
        $package = $composer->getRepositoryManager()->getLocalRepository()->findPackage('celerrate/celerrate', '*');
        if ($package === null) {
            return;
        }
        $packageDirectory = $composer->getInstallationManager()->getInstallPath($package);
        if (!is_string($packageDirectory) || $packageDirectory === '') {
            return;
        }
        $binaryPath = $packageDirectory . '/bin-cache/' . Platform::binaryFileName($triple);
        if (is_file($binaryPath)) {
            return;
        }
        $override = getenv('CELERRATE_DOWNLOAD_BASE_URL');
        $baseUrl = ReleaseUrl::baseUrl(
            $package->getPrettyVersion(),
            is_string($override) && $override !== '' ? $override : null
        );
        if ($baseUrl === null) {
            throw new \RuntimeException(
                'celerrate: version ' . $package->getPrettyVersion()
                . ' is a development version with no released binary; require a tagged release,'
                . ' or point CELERRATE_BINARY at an existing binary.'
            );
        }
        $archiveName = Platform::archiveFileName($triple);
        $downloader = new HttpDownloader($io, $composer->getConfig());
        $io->write("celerrate: downloading {$baseUrl}/{$archiveName}");
        $archiveBody = (string) $downloader->get("{$baseUrl}/{$archiveName}")->getBody();
        $sums = (string) $downloader->get("{$baseUrl}/SHA256SUMS")->getBody();
        $expected = Checksum::expectedFor($archiveName, $sums);
        if ($expected === null) {
            throw new \RuntimeException(
                "celerrate: SHA256SUMS has no entry for {$archiveName}; refusing to install an unverified binary."
            );
        }
        $workingDirectory = $packageDirectory . '/bin-cache.download';
        self::removeDirectory($workingDirectory);
        if (!mkdir($workingDirectory, 0755, true)) {
            throw new \RuntimeException("celerrate: cannot create {$workingDirectory}");
        }
        try {
            $archivePath = $workingDirectory . '/' . $archiveName;
            file_put_contents($archivePath, $archiveBody);
            if (!Checksum::matches($archivePath, $expected)) {
                throw new \RuntimeException(
                    "celerrate: checksum verification failed for {$archiveName}; refusing to install."
                );
            }
            $extracted = Archive::extractBinary($archivePath, $triple, $workingDirectory);
            $binaryDirectory = dirname($binaryPath);
            if (!is_dir($binaryDirectory) && !mkdir($binaryDirectory, 0755, true)) {
                throw new \RuntimeException("celerrate: cannot create {$binaryDirectory}");
            }
            if (!rename($extracted, $binaryPath)) {
                throw new \RuntimeException("celerrate: cannot move the binary into {$binaryPath}");
            }
            chmod($binaryPath, 0755);
        } finally {
            self::removeDirectory($workingDirectory);
        }
        $io->write("celerrate: installed the {$triple} binary");
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
