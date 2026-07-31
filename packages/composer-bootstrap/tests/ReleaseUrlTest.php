<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\ReleaseUrl;
use PHPUnit\Framework\TestCase;

final class ReleaseUrlTest extends TestCase
{
    public function testBuildsTheGithubReleaseBaseForATaggedVersion(): void
    {
        self::assertSame(
            'https://github.com/celerrate/celerrate/releases/download/v0.1.0',
            ReleaseUrl::baseUrl('0.1.0', null)
        );
    }

    public function testDoesNotDoubleALeadingV(): void
    {
        self::assertSame(
            'https://github.com/celerrate/celerrate/releases/download/v0.1.0',
            ReleaseUrl::baseUrl('v0.1.0', null)
        );
    }

    public function testTheOverrideWinsAndLosesItsTrailingSlash(): void
    {
        self::assertSame(
            'http://127.0.0.1:8737',
            ReleaseUrl::baseUrl('0.1.0', 'http://127.0.0.1:8737/')
        );
        self::assertSame(
            'http://127.0.0.1:8737',
            ReleaseUrl::baseUrl('dev-main', 'http://127.0.0.1:8737')
        );
    }

    public function testDevelopmentVersionsHaveNoReleaseToDownloadFrom(): void
    {
        self::assertNull(ReleaseUrl::baseUrl('dev-main', null));
        self::assertNull(ReleaseUrl::baseUrl('0.1.x-dev', null));
    }
}
