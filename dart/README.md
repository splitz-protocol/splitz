# splitz_core

The Dart reference implementation of the splitz shared-bill protocol. One specification,
two implementations, one corpus that both run.

See the [repository](https://github.com/KamaIOps/Splitz-protocol) for `SPEC.md`,
`INTEGRATING.md` and the conformance vectors.

## Conformance

The corpus lives at the repository root, one level above this package, so a
published package cannot carry it — and this package does not ship `test/`
either, because a suite with no corpus to run asserts nothing and the runner
reports it as a pass with skips.

**Conformance is run from a checkout of the repository**, where the suite and
the corpus sit together:

```
git clone https://github.com/KamaIOps/Splitz-protocol
cd Splitz-protocol/dart && dart test
```

`SPLITZ_VECTORS` points the suite at a corpus somewhere other than
`../vectors` when the two are not adjacent.

## Licence

MIT or Apache-2.0, at your option.
