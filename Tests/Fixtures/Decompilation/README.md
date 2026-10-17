# Decompilation quality corpus

The C, Objective-C, C++, and Objective-C++ sources in this directory were
written for HindSight's decompiler corpus and are released into the public
domain. They contain control flow, calls, dynamic dispatch, and integer data
flow. The runner compiles each source at `-O0` and `-O2`, then repeats each
build after stripping local symbols and debug data.

The checks measure functions, CFG blocks, decoder coverage, reconstructed
pseudocode presence, and parameter-name use in address-bearing function
statements. They deliberately avoid exact pseudocode text comparisons.
The Objective-C fixtures require the macOS SDK and Foundation framework.

Run the corpus on macOS after building the analysis worker:

```sh
cargo build -p reworkbench-worker --manifest-path rust/Cargo.toml
python3 scripts/run-decompilation-corpus.py --worker rust/target/debug/reworker
```
