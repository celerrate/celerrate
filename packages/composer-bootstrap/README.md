# celerrate/celerrate

The Composer bootstrap package that installs the platform's `celerrate` binary and exposes `vendor/bin/celerrate`.

## Install

```bash
composer require --dev celerrate/celerrate
```

## How it works

On install, the plugin downloads the binary matching the package version from the corresponding GitHub release. It verifies the downloaded archive's SHA-256 checksum against the release's `SHA256SUMS` file. A mismatch is refused: the plugin aborts rather than install an unverified binary.

## Development

Development happens in the monorepo, [https://github.com/celerrate/celerrate](https://github.com/celerrate/celerrate), directory `packages/composer-bootstrap/`. This repository is a read-only split: issues and pull requests go to the monorepo, not here.

## License

MIT OR Apache-2.0
