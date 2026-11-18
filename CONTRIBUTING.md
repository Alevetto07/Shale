# Contributing to Shale

Thank you for considering a contribution. Shale uses a **Developer Certificate of Origin (DCO)** instead of a contributor licence agreement (CLA).

## Developer Certificate of Origin

By contributing to this repository you certify that:

1. The contribution was created in whole or in part by you and you have the right to submit it under the open source licence indicated in the file; or
2. The contribution is based upon previous work that, to the best of your knowledge, is covered under an appropriate open source licence and you have the right under that licence to submit that work with modifications, whether created in whole or in part by you, under the same open source licence (unless you are permitted to submit under a different licence); or
3. The contribution was provided directly to you by some other person who certified (1), (2) or (4) and you have not modified it; or
4. The contribution is yours, was not copied from somewhere else, and you have the right to submit it.

The text above is derived from [developercertificate.org](https://developercertificate.org/) version 1.1.

## Signed-off-by

Every commit must include a `Signed-off-by` line that matches the commit author:

```text
feat(core): explain what changed

Signed-off-by: Your Name <you@example.com>
```

Git can add this automatically:

```bash
git commit -s -m "feat(core): explain what changed"
```

Pull requests without `Signed-off-by` on each commit may be asked to amend before merge.

## Licence

Contributions are licensed under the same terms as the project: **AGPL-3.0-or-later** (see [LICENSE](LICENSE)).

## Style

- Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`).
- Reference GitLab/GitHub issues with `#123` in commit messages when relevant.
- Run `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings` before opening a pull request when you change Rust code.
- Match existing naming, formatting, and documentation level in the files you touch.

## Questions

Open an issue or ask in the pull request if something is unclear. For licensing questions, see [LICENSING.md](LICENSING.md).