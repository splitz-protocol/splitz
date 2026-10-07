# Splitz-Protocol

Splitz lets friends split a bill and settle it in Zcash. Everyone adds what
they spent; Splitz works out who owes whom, cuts it to the fewest payments,
and gives each payer one Zcash payment request for everything they owe.

It is a library and a specification, not an app: a wallet builds its screens
on it and keeps its own keys, storage and sending (`SPEC.md` §13).

## Features

- Invite people by link, QR code or pasted code (§11).
- Split expenses equally, by amounts, by percentage, by shares or by item (§4).
- Settle with the fewest payments: the true minimum for up to 14 people.
- Get paid in ZEC, in another asset by swap, or in cash (§9.1).
- Pay everyone in one transaction, a swap included (§14.10).
- Close a bill for settling; a changed expense reopens it (§10.9).
- Count a payment only when the person paid confirms it (§10.5).
- Never ask for a debt twice, or for more than is still owed (§14.4).
- Merge a name typed by hand into the person who joined, or take someone off
  (§14.11, §10.8).
- Keep money exact: whole smallest units, no floating point, the same split on
  every device.
- Sync through a relay that only ever holds encrypted entries.

## Try it

With Dart 3.11.4 or later:

```
git clone https://github.com/KamaIOps/Splitz-Protocol.git
cd Splitz-Protocol/dart
dart run example/dinner.dart
```

It prints a shared bill, the fewest payments that settle it, and the one
payment request a payer signs. For the full app, see
[vizor-wallet-splits](https://github.com/KamaIOps/vizor-wallet-splits).

## Use It in a Wallet

A wallet provides seven things: sending, a secret store, storage, a relay,
exchange rates, swaps and its account details (§15). `INTEGRATING.md` walks
through the rest, with samples in Kotlin, Dart and JavaScript.

Nothing is on a package registry yet; depend on this repository by commit.
Dart needs both packages pinned to the same **full 40-character commit sha**:

```yaml
dependencies:
  splitz_core:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: dart
      ref: <full commit sha>
  splitz_host:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: splitz_host
      ref: <full commit sha>
```

```toml
[dependencies]
splitz-core = { git = "https://github.com/KamaIOps/Splitz-Protocol", rev = "<commit sha>" }
splitz-host = { git = "https://github.com/KamaIOps/Splitz-Protocol", rev = "<commit sha>" }
```

Kotlin, Swift and JavaScript use the binding (`rust/splitz-ffi`), packaged by
`tools/package/{android,ios,npm}.sh`.

## Repository

| | |
|---|---|
| `dart/`, `rust/splitz-core` | the protocol, in Dart and in Rust |
| `splitz_host/`, `rust/splitz-host` | the wallet layer: signing, storage, sync, swaps |
| `rust/splitz-ffi`, `tools/package` | the binding and its packages |
| `tools/relay` | the relay |
| `vectors/` | 1016 cases in 24 files, in no particular language |
| `tools/` | the checks beyond the test cases |

## Development

Needs Dart 3.11.4+ and Rust 1.88+:

```
cd dart && dart test
cd splitz_host && dart test
cd rust && cargo test
```

The Dart and Rust implementations are written separately and held to the same
answers on generated inputs (`tools/differential`). The test cases come from a
third implementation written only from the spec. Further checks: the spec's
figures against the code (`tools/spec`), outside readers (`tools/oracle`), a
whole bill from Kotlin, Swift, Dart and JavaScript (`tools/ffi`), and real
money on a local chain (`tools/regtest/run.sh prove`, with Docker).

## Conformance

An implementation conforms when it reproduces every case in `vectors/` and
follows §14. `SPEC.md` §12 lists 96 reasons the library refuses something,
each with a test case except `bill_not_scalar_values`, which each
implementation tests in its own suite. See `CONFORMANCE.md`.

## Docs

- `SPEC.md`: the protocol.
- `INTEGRATING.md`: building a wallet on it.
- `CONFORMANCE.md`: what conformance means.
- `vectors/README.md`: the test case format.

## License

MIT or Apache-2.0, at your choice. See `LICENSE-MIT` and `LICENSE-APACHE`.
