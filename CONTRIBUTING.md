# Contributing to keeper

## License

keeper is licensed under the [Apache License 2.0](LICENSE). Anything you submit for inclusion is
licensed under the same terms (Apache-2.0 §5), including its patent grant. There is no separate
contributor license agreement.

## Sign your commits (DCO)

Every commit in a pull request carries a sign-off: your statement that you wrote the change, or
otherwise have the right to submit it under the project's license, as the
[Developer Certificate of Origin 1.1](DCO) puts it. The `DCO` check fails a pull request with an
unsigned commit.

Sign off by committing with `-s`:

```sh
git commit -s -m "fix timeline pagination"
```

Git appends a line with the name and email of your git identity:

```
Signed-off-by: Ada Lovelace <ada@example.org>
```

- The email must be the commit's **author** email. GitHub's `…@users.noreply.github.com` address
  is fine if you commit with it.
- Use a name you are known by. It does not have to be your legal name, but it must not be anonymous.
- Commits made in GitHub's web editor are signed off automatically.
- Editors can do it for you: VS Code `"git.alwaysSignOff": true`; JetBrains IDEs have a
  *Sign-off commit* option in the commit dialog. For the command line,
  `git config --global alias.cs "commit -s"`.
- Code written with an AI assistant is covered by the same sign-off: you certify that you have the
  right to submit it, so do not submit generated code that reproduces someone else's code under an
  incompatible license.

### Forgot the sign-off?

The failing check prints the exact commands for your pull request. Either rewrite the branch:

```sh
git rebase --signoff <base>      # the base commit the check names
git push --force-with-lease
```

or, if you would rather not rewrite history, push one empty, signed-off remediation commit with
one line per unsigned commit:

```sh
git commit --allow-empty -s -m "DCO remediation" \
  -m "I, Ada Lovelace <ada@example.org>, hereby add my Signed-off-by to this commit: <full sha>"
```

A remediation commit counts only for commits with the same author email as the remediation
commit itself.

## Provenance

- Dependencies (Rust and npm) must be permissively licensed (Apache-2.0, MIT, BSD, ISC, Zlib,
  MPL-2.0…). `cargo deny check` and `bun run check:licenses` enforce it.
- GPL/AGPL projects (e.g. Element X, gomuks) may be studied for patterns. Never copy their code.
- Ported permissive code names its source and license, and keeps its notices.
- Never commit secrets. See `docs/credentials.md`.

## Before you open a pull request

Run `bun run check:all`, or the narrower gate for what you changed (see `AGENTS.md`, *Quality
Gates*), and fill in the pull request template.
