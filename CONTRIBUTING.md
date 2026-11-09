# Contributing

Thanks for helping. This project is unofficial and community-maintained; the
bar is that it stays an exact, safe mirror of the Henrri v1 API.

## The rules the code follows

- **One route, one tool.** Every operation of `spec/operations.tsv` maps to one
  tool in `src/coverage.rs`. A test fails when the table, the registered tools
  or the specification disagree.
- **A test for every quirk.** When the real API behaves differently from its
  documentation, handle it once, in the client or the tool, and pin it with a
  test in `tests/api_contract.rs` against the fake in `tests/common`.
- **Fail closed.** A tool that cannot be undone takes `confirm` and refuses
  without it; it is annotated `destructiveHint` when it destroys or locks data.
- **The description is the prompt.** A tool's doc comment is what a model
  reads: say what it does, what it needs, and what it costs.
- No `unwrap`, `expect` or `panic` outside tests; `cargo clippy` with the
  pedantic group must pass with `-D warnings`.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

If you have sandbox credentials, also run the live tests
(`HENRRI_SANDBOX_TESTS=1 cargo test --test sandbox`, see the README). Never
commit credentials: `.env` is git-ignored for that.

## When Henrri changes its API

`scripts/spec-operations.sh` prints the live operations. If CI's daily spec
check fails, regenerate `spec/operations.tsv`, then add or adjust the tool and
its row in `src/coverage.rs`.
