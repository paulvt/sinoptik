# Changelog

All notable changes to Sinoptik will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://git.luon.net/paul/sinoptik/compare/v0.2.2...HEAD
[0.2.2]: https://git.luon.net/paul/sinoptik/compare/v0.2.1...v0.2.2
[0.2.1]: https://git.luon.net/paul/sinoptik/compare/v0.2.0...v0.2.1
[0.2.0]: https://git.luon.net/paul/sinoptik/compare/v0.1.0...v0.2.0
[0.1.0]: https://git.luon.net/paul/sinoptik/commits/tag/v0.1.0
