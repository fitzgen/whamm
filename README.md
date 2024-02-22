# Instrumentation DSL #

This DSL is inspired by the dtrace D language.

## Tutorials ##

To run basic build:
```shell
cargo build
```

To run tests:
```shell
cargo test
cargo test -- --nocapture # With stdout tracing
```

To run project (there are example dscripts in `tests/dscripts` folder):
```shell
cargo run <path_to_dscript>
```