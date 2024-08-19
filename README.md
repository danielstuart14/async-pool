# async-pool

[![Documentation](https://docs.rs/async-pool/badge.svg)](https://docs.rs/async-pool)

Statically allocated pool providing a std-like Box, with hability to asynchronously wait for a pool slot to become available. 

This crate is tailored to be used with no-std async runtimes, like [Embassy](https://embassy.dev/), but can also be used in std environments (check examples).

## Dependencies 

This crate uses the `AtomicWaker` functionality from `embassy-sync` crate, which in turn requires a critical section implementation. Check [critical-section](https://crates.io/crates/critical-section).

## Previous work

This crate is heavily based on [atomic-pool](https://github.com/embassy-rs/atomic-pool).

## License

This work is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
