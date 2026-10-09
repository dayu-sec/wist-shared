# Changelog

All notable changes to `wist-shared` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-10-09

### Added

- New `protocol` module with the cross-process **error projection** wire types:
  `ProtocolError`, `Severity`, and `ProtocolErrorEnvelope`
  (`{ "error": { code, message, … } }`). Shared by the gateway / control HTTP APIs so they
  project structured errors through one stable envelope instead of leaking internal
  detail. Optional fields (`retryable` / `severity` / `correlation_id` / `fields`) are
  additive and omitted when unset.
