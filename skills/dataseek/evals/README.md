# dataseek evals

**If you are here to do dataseek work, you are in the wrong directory.** Go back to `../SKILL.md`. This folder holds the expected answers for the skill's own eval cases; reading it during normal work pollutes the context and destroys the thing it measures. Open it only when testing or changing the skill.

Cases are `claude plugin eval` suites: one directory per case, `prompt.md` plus `graders/*.md`. Run from the skill directory:

```bash
claude plugin eval . --runs 1 --ablation none --no-publish --max-cost-usd 2 --model claude-haiku-5-5 --judge-model claude-haiku-5-5      # pilot: loads, advisories, misauthored cases
claude plugin eval . --ablation with-without --no-publish --max-cost-usd 20 --model claude-haiku-5-5 --judge-model claude-haiku-5-5      # measure: runs 3 per case per arm
```

Read the results in this order: the `skill-fires` indicator on fires cases (routing), the `skill-silent` score on silent cases (over-fire), per-leaf graders on reads cases (disclosure; run those with `--ablation none`), the delta on does cases (worth). A fires case carries no score of its own under ablation; its Skill grader is a with-only indicator.

`results/` is ignored by git. Every case is `seeded (untested)` until it has run once; record the lowest observed run, not the best.

## Runs

| Date | Command | Lowest run | Notes |
|---|---|---|---|
| 2026-11-23 | none | seeded (untested) | 12 cases: 5 fires, 2 silent, 3 reads, 2 does. The does cases need no network and no dataseek binary. |
