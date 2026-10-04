# CI workflow (to be installed)

`check.yml` is the GitHub Actions workflow for this repo: `cargo fmt --check`, `cargo clippy
-D warnings`, `cargo test --release`, and a hygiene job (no key-shaped files, honest-label markers
present in `SPEC.md` / `README.md`).

It lives here instead of `.github/workflows/` only because the dev session's GitHub token lacks the
`workflow` OAuth scope needed to push workflow files. To install:

```bash
mkdir -p .github/workflows && git mv ci/check.yml .github/workflows/check.yml
```
