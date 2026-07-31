<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/** Extraction of the release archives (PharData: no ext-zip needed). */
final class Archive
{
    /**
     * Extracts a release archive and returns the path of the binary it
     * must contain at celerrate-<triple>/<binary name>.
     */
    public static function extractBinary(string $archivePath, string $targetTriple, string $destinationDirectory): string
    {
        if (!is_dir($destinationDirectory) && !mkdir($destinationDirectory, 0755, true)) {
            throw new \RuntimeException("celerrate: cannot create {$destinationDirectory}");
        }
        if (substr($archivePath, -7) === '.tar.gz') {
            $tarPath = self::inflateToUniqueTar($archivePath, $destinationDirectory);
            try {
                $archive = new \PharData($tarPath);
                $archive->extractTo($destinationDirectory, null, true);
            } finally {
                unlink($tarPath);
            }
        } else {
            $archive = new \PharData($archivePath);
            $archive->extractTo($destinationDirectory, null, true);
        }
        $binaryPath = $destinationDirectory . '/celerrate-' . $targetTriple . '/' . Platform::binaryFileName($targetTriple);
        if (!is_file($binaryPath)) {
            throw new \RuntimeException(
                "celerrate: the archive does not contain celerrate-{$targetTriple}/" . Platform::binaryFileName($targetTriple)
            );
        }
        chmod($binaryPath, 0755);
        return $binaryPath;
    }

    /**
     * Inflates a .tar.gz to a plain .tar at a fresh, never-before-used
     * path. PharData::decompress() is deliberately avoided: within a
     * single PHP process, ext-phar keeps a process-wide registry of
     * every path it has ever opened a Phar/PharData for, and reusing
     * one (as decompress() does, always targeting the sibling .tar
     * name) either throws "a phar with that name already exists" or,
     * with a custom extension, silently extracts corrupted content.
     * Plain zlib functions never touch that registry.
     */
    private static function inflateToUniqueTar(string $archivePath, string $destinationDirectory): string
    {
        $source = @gzopen($archivePath, 'rb');
        if ($source === false) {
            throw new \RuntimeException("celerrate: cannot open {$archivePath}");
        }
        $tarPath = $destinationDirectory . '/.inflated-' . bin2hex(random_bytes(8)) . '.tar';
        $destination = @fopen($tarPath, 'wb');
        if ($destination === false) {
            gzclose($source);
            throw new \RuntimeException("celerrate: cannot create {$tarPath}");
        }
        while (!gzeof($source)) {
            $chunk = gzread($source, 1024 * 1024);
            if ($chunk === false) {
                fclose($destination);
                gzclose($source);
                unlink($tarPath);
                throw new \RuntimeException("celerrate: cannot read {$archivePath} (corrupt gzip stream?)");
            }
            if (fwrite($destination, $chunk) !== strlen($chunk)) {
                fclose($destination);
                gzclose($source);
                unlink($tarPath);
                throw new \RuntimeException("celerrate: cannot write {$tarPath} (disk full?)");
            }
        }
        fclose($destination);
        gzclose($source);
        return $tarPath;
    }
}
