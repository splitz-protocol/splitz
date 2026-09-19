# splitz

The Rust implementation of the splitz shared-bill protocol. One specification,
two implementations, one corpus that both run.

See the [repository](https://github.com/KamaIOps/Splitz-protocol) for `SPEC.md`,
`INTEGRATING.md` and the conformance vectors.

## Conformance

The corpus lives at the repository root, one level above this package, so a
published crate cannot carry it. `cargo test` skips the conformance suite when
it is absent; point `SPLITZ_VECTORS` at a checkout to run it:

```
SPLITZ_VECTORS=/path/to/Splitz-protocol/vectors cargo test
```

## Licence

MIT or Apache-2.0, at your option.
