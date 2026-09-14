# wist-shared

Shared runtime helpers: filesystem, IDs, paths, time, and primitives.

[![crates.io](https://img.shields.io/crates/v/wist-shared.svg)](https://crates.io/crates/wist-shared)
[![docs.rs](https://img.shields.io/docsrs/wist-shared/latest.svg)](https://docs.rs/wist-shared)
[![Downloads](https://img.shields.io/crates/d/wist-shared.svg)](https://crates.io/crates/wist-shared)
[![MSRV](https://img.shields.io/badge/rustc-1.85+-orange.svg)](#)
[![CI](https://github.com/dayu-sec/wist-shared/actions/workflows/ci.yml/badge.svg)](https://github.com/dayu-sec/wist-shared/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/dayu-sec/wist-shared/branch/main/graph/badge.svg)](https://codecov.io/gh/dayu-sec/wist-shared)
[![dependency status](https://deps.rs/repo/github/dayu-sec/wist-shared/status.svg)](https://deps.rs/repo/github/dayu-sec/wist-shared)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`wist-shared` is the small, dependency-light toolbox used across the `wist-*` crates. It has no
internal dependencies, so it can be used from any component.

## Modules

| Module        | Purpose                                                        |
| ------------- | -------------------------------------------------------------- |
| `fs`          | Filesystem helpers (atomic JSON / byte writes).                |
| `ids`         | Shared ID helpers.                                             |
| `paths`       | Well-known file and directory names (workdir files, config, …).|
| `time`        | RFC 3339 timestamps and time helpers.                          |
| `primitives`  | Cross-domain scalar wrappers (`DateTime`, `Secret`, `Int`, `Bool`, `Float`). |
| `integrity`   | Development-only local integrity helpers.                      |
| `error_codes` | Shared error-code placeholders.                                |

## Example

```rust
use wist_shared::fs::write_json_atomic;
use wist_shared::time::now_rfc3339;

write_json_atomic(path, &value)?;
let now = now_rfc3339();
```

## Related crates

- [`wist-contracts`](../wist-contracts) — schema objects this crate helps read and write.
- [`wist-agentd`](../wist-agentd) — the primary consumer.

## License

[Apache-2.0](LICENSE)
