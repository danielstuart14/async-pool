# async-pool

[![Documentation](https://docs.rs/async-pool/badge.svg)](https://docs.rs/async-pool)

Statically allocated pool providing a std-like Box, allowing to asynchronously await for a pool slot to become available. 

It is tailored to be used with no-std async runtimes, like [Embassy](https://embassy.dev/), but can also be used in std environments (check examples). 

The most common use-case is sharing large memory regions on constrained devices (e.g. microcontrollers), where multiple tasks may need to use the memory for buffering an I/O or performing calculations, and having separate static buffers would be too costly.

It is important to know that waiting forever for a memory slot to be available may dead-lock your code if done wrong. With that in mind, you should consider using a timeout when allocating asynchronously (e.g. [embassy_time::with_timeout](https://docs.rs/embassy-time/0.3.2/embassy_time/fn.with_timeout.html)).

## Dependencies 

This crate requires a critical section implementation. Check [critical-section](https://crates.io/crates/critical-section).

## Example

```
use async_pool::{pool, Box};

struct Buffer([u8; 256]);

// A maximum of 2 Packet instances can be allocated at a time.
// A maximum of 3 futures can be waiting at a time.
pool!(BufferPool: [Buffer; 2], 3);

async fn run() {
    // Allocate non-blocking (will return None if no data slot is available)
    let box1 = Box::<BufferPool>::new(Buffer([0; 256]));

    // Allocate asynchronously (will wait if no data slot is available)
    // This can return None if all future slots are taken
    let box2 = Box::<BufferPool>::new_async(Buffer([0; 256])).await;
}
```

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
