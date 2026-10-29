# Security policy

**Status:** design draft for review · decided, not built

## Scope, honestly stated

This repository is a design specification. **No code exists**, so there is no running software
to exploit and no binary to patch. What can be wrong today is the design itself: a flaw in the
threat model, a hole in the erasure boundary, a chain-format weakness, a tenancy bypass that the
spec fails to close. Those are welcome as **public issues** — a design flaw found now is the
cheapest kind, and hiding it would only delay the fix. Use the design-review-finding issue
template and name the decision handle the flaw sits under.

Private reporting applies once code lands: from the first tagged implementation release onward,
a vulnerability in released code goes through the private channel below, not a public issue.

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
  written against is a hostile co-tenant, so a finding that breaks tenant isolation, the signed
  journal chain, or the erasure boundary is treated as the most severe class.
- A fix timeline agreed with the reporter before any public disclosure. During the design phase
  the "fix" is an amended ADR and corrected pages; the reporter is credited in the change unless
  they ask otherwise.

## Advisories and CVEs

Once binaries ship, the project will publish **GitHub Security Advisories** for vulnerabilities
in released code and request CVE identifiers through GitHub's CVE Numbering Authority for any
vulnerability that affects a tagged release. Design-phase findings do not receive CVEs — there is
no affected software version to name.

## Where the security design lives

The security posture this policy defends is specified in
[docs/architecture/security.md](docs/architecture/security.md): the tenancy model, the key
custody map, the controls on privileged services, and the threat model. A report that argues
against one of those decisions should name its decision handle, so the discussion lands on the
page that owns it.
