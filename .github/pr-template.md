## Summary

<!-- What changes for someone running dsk, and why. Link the issue if there is one. -->

## Release line

<!-- The conventional title this lands under, e.g. `fix: ...`. It becomes the changelog line. -->

## Checklist

- [ ] `just ci` passes
- [ ] Tests cover the change; a new or changed adapter has a `parse` test against a recorded fixture
- [ ] Changed snapshots were stepped through with `just review`
- [ ] Docs are current: `docs/reference/contract.md` for flags or output, an ADR for a decision, ADR 0004 and `docs/synthesis/terms.md` for a new source
- [ ] Re-recorded fixtures are trimmed, carry no key, and have every personal e-mail address replaced
