# Changelog

All notable changes to Sinoptik will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.6]

### Added

* Add full release Gitea Actions workflow

### Changed

* Simplify Gitea Actions check, lint and test workflow
* Improve no known map colors found error description

### Fixed

* Update coordinates of Eindhoven in tests (Nomatim changed its geocoding)
* Increase sampling area to 31×31 pixels (#26)
* Switch to new Buienradar color scheme/maps key (#27)

## [0.2.5]

### Added

* Add Gitea Actions workflow for cargo

### Changed

* Updated dependencies on `cached`, `chrono-tz` and `geocoding`

### Fixed

* Fix float comparison in tests
* Fix clippy issues

### Security

* Update dependencies ([RUSTSEC-2023-0018](https://rustsec.org/advisories/RUSTSEC-2023-0018.html))

## [0.2.4] - 2022-07-05

### Added

* Add proper error handling and show them via the API (#25)

### Changed

* Run map refresher as an ad hoc liftoff fairing in Rocket
* Changed emojis in log output

### Removed

* Removed `AQI_max` and `pollen_max` from the forecast JSON introduced in
  version 0.2.0

### Fixed

* Verify sample coordinate bounds (#24)
* Default to current time if `Last-Modified` HTTP header is missing for
  retrieved maps

## [0.2.3] - 2022-05-21

### Fixed

* Update the examples in `README.md`
* Fix tests by adding missing type
* Fix map key color code for level 8 used by map sampling

## [0.2.2] - 2022-05-10

### Changed

* Switch to Rocket 0.5 RC2

### Fixed

* Fix timestamps for map samples not being correct (AQI, PAQI, UVI metrics) (#22)
* Valid samples/items will no longer be discarded too early

## [0.2.1] - 2022-05-08

### Added

* Add tests for the merge functionality of the combined provider (PAQI)

### Fixed

* Filter out old item/samples in combined provider (PAQI)

## [0.2.0] - 2022-05-07

### Added

* Add `AQI_max` and `pollen_max` to the forecast JSON (only when the PAQI
  metric is selected) (#20)

## [0.1.0] - 2022-03-07

Initial release.

[Unreleased]: https://git.luon.net/paul/sinoptik/compare/v0.2.6...HEAD
[0.2.6]: https://git.luon.net/paul/sinoptik/compare/v0.2.5...v0.2.6
[0.2.5]: https://git.luon.net/paul/sinoptik/compare/v0.2.4...v0.2.5
[0.2.4]: https://git.luon.net/paul/sinoptik/compare/v0.2.3...v0.2.4
[0.2.3]: https://git.luon.net/paul/sinoptik/compare/v0.2.2...v0.2.3
[0.2.2]: https://git.luon.net/paul/sinoptik/compare/v0.2.1...v0.2.2
[0.2.1]: https://git.luon.net/paul/sinoptik/compare/v0.2.0...v0.2.1
[0.2.0]: https://git.luon.net/paul/sinoptik/compare/v0.1.0...v0.2.0
[0.1.0]: https://git.luon.net/paul/sinoptik/commits/tag/v0.1.0
