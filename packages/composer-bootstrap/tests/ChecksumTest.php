<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Checksum;
use PHPUnit\Framework\TestCase;

final class ChecksumTest extends TestCase
{
    private const EMPTY_HASH = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

    public function testFindsTheHashForAFileNameInASumsBody(): void
    {
        $sums = self::EMPTY_HASH . "  celerrate-a.tar.gz\n"
            . str_repeat('a', 64) . "  celerrate-b.tar.gz\n";
        self::assertSame(self::EMPTY_HASH, Checksum::expectedFor('celerrate-a.tar.gz', $sums));
        self::assertSame(str_repeat('a', 64), Checksum::expectedFor('celerrate-b.tar.gz', $sums));
    }

    public function testReturnsNullWhenTheFileHasNoEntry(): void
    {
        self::assertNull(Checksum::expectedFor('celerrate-c.tar.gz', self::EMPTY_HASH . "  celerrate-a.tar.gz\n"));
        self::assertNull(Checksum::expectedFor('celerrate-a.tar.gz', ''));
    }

    public function testMatchesComparesTheFileAgainstTheExpectedHash(): void
    {
        $path = tempnam(sys_get_temp_dir(), 'celerrate-checksum-test');
        self::assertIsString($path);
        file_put_contents($path, '');
        self::assertTrue(Checksum::matches($path, self::EMPTY_HASH));
        self::assertFalse(Checksum::matches($path, str_repeat('0', 64)));
        unlink($path);
    }
}
