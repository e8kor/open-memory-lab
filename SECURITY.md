# Security policy

## Supported versions

Security fixes land on the latest release. Please upgrade to the newest version on the
[releases page](https://github.com/e8kor/open-memory-lab/releases) before reporting.

## Please don't report in public

Report a vulnerability privately, never in a public issue, pull request or Discussion. That
includes flaws in tenant isolation, key handling, the tamper-evident history, erasure, and the
review rules.

## Reporting channel

Report privately via **GitHub private vulnerability reporting** on
[e8kor/open-memory-lab](https://github.com/e8kor/open-memory-lab/security/advisories/new)
("Report a vulnerability" under the repository's Security tab). This channel also serves as the
project's private contact for conduct matters (see
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)).

## What to expect

- **Acknowledgement within 7 days** of a private report.
- Triage against the threat model in
  [docs/architecture/security.md](docs/architecture/security.md) — the attacker that page is
  written against is a hostile co-tenant, so a finding that breaks tenant isolation, the
  tamper-evident history, or the erasure boundary is treated as the most severe class.
- A fix timeline agreed with the reporter before any public disclosure. The reporter is credited
  in the advisory unless they ask otherwise.

## Advisories and CVEs

The project publishes **GitHub Security Advisories** for vulnerabilities in released code and
requests CVE identifiers through GitHub's CVE Numbering Authority for any vulnerability that
affects a tagged release.

## Security model

The security model is documented in [docs/architecture/security.md](docs/architecture/security.md):
the tenancy model, key custody, the controls on privileged services, and the threat model.
