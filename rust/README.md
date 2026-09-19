# splitz

The Rust implementation of the splitz shared-bill protocol. One specification,
two implementations, one corpus that both run.

See the [repository](https://github.com/KamaIOps/Splitz-protocol) for `SPEC.md`,
`INTEGRATING.md` and the conformance vectors.

## Conformance

The corpus lives at the repository root, one level above this package, so a
published crate cannot carry it — and this package does not ship `tests/`
either, because a suite with no corpus to run asserts nothing and `cargo test`
has no state in which to say so. It would print `ok`.

**Conformance is run from a checkout of the repository**, where the suite and
the corpus sit together:

```
git clone https://github.com/KamaIOps/Splitz-protocol
cd Splitz-protocol/rust && cargo test
```

`SPLITZ_VECTORS` points the suite at a corpus somewhere other than `../vectors`
when the two are not adjacent. In a checkout, an absent corpus **fails** the
suite rather than skipping it.

## Licence

MIT or Apache-2.0, at your option.
