# Contributing

Thanks for your interest in contributing to this project.

## Before you open a pull request: sign the CLA

Every contribution requires a signed Contributor License Agreement. Read it
at [`CLA.md`](CLA.md) in this repo before you start writing code.

When you open your first pull request, the CLA Assistant bot will comment
with instructions to sign electronically. Sign once and it's recorded for
future contributions across the org.

## Questions

Open an issue, or find us on [Discord](https://discord.gg/6qsdhSPE).

## Building and testing

```bash
make test       # cargo test
make ci         # fmt-check, clippy, test, cargo-deny, unsafe-comment audit
```

`make ci` is the gate to run before opening a pull request. The other targets are
listed by `make help`.

### Signed test builds on macOS

Do not run unsigned development builds against a saved Linear key. Repeated
unsigned builds can cause a new Keychain prompt for the saved Linear
credential. For live development testing, the owner can create a local code
signing certificate in Keychain Access and keep its private key on this Mac.
After each build, sign the executable with the same identity and the fixed
`org.agent-ix.ix-board.test` identifier:

```bash
cargo build
IX_BOARD_SIGNING_IDENTITY="<local code signing identity>" \
  bash scripts/sign_test_build.sh target/debug/ix-board
```

The script verifies the signature and prints its designated requirement. Run
the signed binary for `auth status` and grant its first Keychain request only
after checking the signing identity. Rebuilds overwrite the signature, so sign
each new build before using the saved credential. If the same identity still
causes repeated prompts, stop live dev-binary tests and report the signature
and prompt behavior without disclosing the credential. No signing identity is
created or imported by the script.
