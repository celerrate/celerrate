<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

use Composer\Composer;
use Composer\EventDispatcher\EventSubscriberInterface;
use Composer\IO\IOInterface;
use Composer\Plugin\PluginInterface;
use Composer\Script\ScriptEvents;

/** The composer-plugin entry point declared in extra.class. */
final class Plugin implements PluginInterface, EventSubscriberInterface
{
    /** @var Composer */
    private $composer;

    /** @var IOInterface */
    private $io;

    public function activate(Composer $composer, IOInterface $io): void
    {
        $this->composer = $composer;
        $this->io = $io;
    }

    public function deactivate(Composer $composer, IOInterface $io): void
    {
    }

    public function uninstall(Composer $composer, IOInterface $io): void
    {
    }

    public static function getSubscribedEvents(): array
    {
        return [
            ScriptEvents::POST_INSTALL_CMD => 'installBinary',
            ScriptEvents::POST_UPDATE_CMD => 'installBinary',
        ];
    }

    public function installBinary(): void
    {
        (new BinaryInstaller())->install($this->composer, $this->io);
    }
}
