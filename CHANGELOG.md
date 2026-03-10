# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7](https://github.com/nownabe/chikuwa/compare/v0.1.6...v0.1.7) - 2026-03-10

### Added

- add --version flag to CLI ([#39](https://github.com/nownabe/chikuwa/pull/39))
- add mouse click support for tree view navigation ([#36](https://github.com/nownabe/chikuwa/pull/36))
- display usage API errors and next-fetch countdown in status bar ([#34](https://github.com/nownabe/chikuwa/pull/34))

### Fixed

- reduce usage API polling frequency and add 429 backoff ([#33](https://github.com/nownabe/chikuwa/pull/33))

## [0.1.6](https://github.com/nownabe/chikuwa/compare/v0.1.5...v0.1.6) - 2026-03-10

### Added

- display Claude API usage gauges in status bar ([#26](https://github.com/nownabe/chikuwa/pull/26))

## [0.1.5](https://github.com/nownabe/chikuwa/compare/v0.1.4...v0.1.5) - 2026-03-09

### Added

- limit tool display to 5 and show tool count ([#20](https://github.com/nownabe/chikuwa/pull/20))

### Fixed

- handle NerdFont icon prefix in nvim pane titles ([#25](https://github.com/nownabe/chikuwa/pull/25))
- check exit status of tmux set-hook in register_hooks ([#18](https://github.com/nownabe/chikuwa/pull/18))
