# Decompilation quality corpus

`darwin_catalog_sample.h/.m` is a CC0 native metadata/declaration qualification fixture. Its C function, custom aggregate, class/instance methods, properties with custom accessors and backing ivars exercise the Darwin collector and Apple Clang-to-worker prototype path. Run `python3 scripts/test-darwin-catalog.py /absolute/path/to/reworker` on macOS; this complements the main source-fidelity corpus.

The C, Objective-C, C++, and Objective-C++ sources in this directory were
written for HindSight's decompiler corpus and are released into the public
domain. They contain control flow, calls, dynamic dispatch, and integer data
flow. The runner compiles each source at `-O0` and `-O2`, then repeats each
build after stripping local symbols and debug data.

The checks measure functions, CFG blocks, decoder coverage, reconstructed
pseudocode presence, expression rows, resolved call-target rows, recovered type
evidence, parameter-name use, and source-address mapping. They deliberately
avoid exact pseudocode text comparisons. Every push and pull request runs the
same corpus on a macOS CI worker.
The Objective-C fixtures require the macOS SDK and Foundation framework.

`nameflow_sample.h/.s` and `nameflow_oracle.c` qualify shared-effect parameter
identities, copy chains, GP/pair/FP frame spills, pre-index writeback and
overlapping-base pair loads. `scripts/test-arm64-nameflow.py` imports their
Apple Clang declarations into the worker, checks actual pseudocode and runs
1,560 original-native result checks on Apple Silicon. This does not execute
reconstructed source or close the full source-variable milestone.

`scripts/test-arm64-vector-extract.py` generates CC0 assembly/C fixtures for
all 82 legal integer `UMOV`/`MOV` and `SMOV` lane forms. Apple Clang assembles
each form; worker expressions/known-bit results and, on Apple Silicon, 42,640
native comparisons check normal non-streaming EL0 results. Intel macOS runs
cross-assembly/worker checks without claiming native execution. This is a
scoped instruction-effect gate, not whole-function source equivalence.

Run the corpus on macOS after building the analysis worker:

```sh
cargo build -p reworkbench-worker --manifest-path rust/Cargo.toml
python3 scripts/run-decompilation-corpus.py --worker rust/target/debug/reworker
```
