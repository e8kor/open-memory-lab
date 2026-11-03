# Contributing to Open Memory Lab

Thank you for helping build Open Memory Lab. Every contribution counts: a bug report, a fix, a
clearer page, a new plugin or SDK feature, or a question that shows us where the docs fall short.

Everyone taking part follows our [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to contribute

- **Report a bug.** Open an issue with the [bug report form](https://github.com/e8kor/open-memory-lab/issues/new?template=bug_report.yml).
  Include the version, how you run OML and the smallest steps that reproduce it.
- **Suggest a feature.** Use the [feature request form](https://github.com/e8kor/open-memory-lab/issues/new?template=feature_request.yml)
  and describe the problem first, then the change you'd like.
- **Ask a question or share an idea.** Start a thread in [Discussions](https://github.com/e8kor/open-memory-lab/discussions).
- **Fix or improve something.** Look for issues labelled `good first issue` or `help wanted`,
  or open a pull request for a fix you've found.
- **Report a vulnerability.** Never in a public issue: follow [SECURITY.md](SECURITY.md).

## Development setup

You need a stable Rust toolchain (via rustup), Docker with the compose plugin, `jq` and `psql`.

```sh
git clone https://github.com/e8kor/open-memory-lab
cd open-memory-lab
docker compose up -d postgres                # a local database for tests
cargo build --workspace
```

Run the same checks as CI before you push:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
DATABASE_URL=postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev cargo test --workspace --locked
bash scripts/poc-demo.sh                     # end-to-end walk through a throwaway database
```

### SDKs and plugins

- Python SDK: `cd sdks/python && uv run --locked pytest`
- TypeScript SDK: `cd sdks/typescript && npm ci && npm test`
- The generated SDK layers are rebuilt with `python3 scripts/generate_sdks.py`, never edited by hand.
- `python3 scripts/check_plugin.py` checks that the plugin and its skill name the tools the server serves.

### Documentation

The documentation site is built strict, so a broken link or a page missing from the navigation
fails the build:

```sh
uvx --with mkdocs-material mkdocs serve          # preview at http://127.0.0.1:8000
uvx --with mkdocs-material mkdocs build --strict # what CI runs
```

If you change the HTTP API, keep [errors.md](docs/api/errors.md), [openapi.yaml](docs/api/openapi.yaml)
and [http-api.md](docs/api/http-api.md) in step. `python scripts/check_contract_sync.py` checks them.

## Pull requests

1. Fork the repository and branch from `main`.
2. Keep each pull request to one change, with tests for new behaviour.
3. Run the checks above, and update the docs when behaviour changes.
4. Fill in the [pull request template](.github/PULL_REQUEST_TEMPLATE.md) and link the issue it closes.

Commit messages use the imperative mood ("Add workarea export", not "Added"). The maintainer
reviews every pull request; [CODEOWNERS](.github/CODEOWNERS) lists who owns which area.

### Changes to how OML works

A change to a core behaviour, such as how memories are reviewed, how history is recorded or
what the agent API promises, starts as an issue or a Discussion so it can be agreed before code
is written. The maintainer records accepted changes in the project's decision log under
[`docs/adr/`](docs/adr/index.md).

## Licence

By contributing, you agree that your contributions are licensed under [Apache-2.0](LICENSE),
and contributions to `docs/` also under [CC-BY-4.0](LICENSE-docs).
