#!/bin/bash

set -euxo pipefail

cargo test
cargo build --features _enable_critical_section --target thumbv6m-none-eabi
cargo build --target thumbv7em-none-eabi
