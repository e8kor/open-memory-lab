Engineering practices (process only, no behaviour change): a number registry (`docs/NUMBERS.md`,
`scripts/reserve_number.py`) with a CI check that no pull request introduces an ADR, FR or
migration number reserved by another branch; a pre-review checklist; lints for migrations, port
5432 defaults and LIKE in candidate SQL; changelog fragments in `changes/`; a weekly dependency
pass; and lane hygiene notes in `CONTRIBUTING.md`.
