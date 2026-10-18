# Releasing

1. Bump `version` in `Cargo.toml` and `desktop/Cargo.toml`, run `cargo build` and
   `cargo build --manifest-path desktop/Cargo.toml` so both lock files follow, commit.
2. Optional, before tagging: `scripts/e2e_update.sh` (needs `minisign`) checks the updater end to
   end against a fake release on your machine.
3. Tag and push: `git tag v0.3.0 && git push origin v0.3.0`.

[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds the four archives, each
attaching itself to the GitHub release as soon as it's built. Once all four are up, the `sign`
job signs each one and uploads `<archive>.minisig`. Installed apps only take a release once its
signature for their platform is there, so a half-published release is never installed. If a
build fails, re-run the failed jobs; `sign` runs again after them.

## The update signing key

Updates are signed with [minisign](https://jedisct1.github.io/minisign/) (Ed25519).

- **Public key:** `packaging/update.pub` (key id `C617FFC25514E420`). It is compiled into every
  build (`include_str!` in `src/update.rs`), and release builds trust nothing else.
- **Secret key:** kept off the repo. The owner's copy is `~/.config/cardboard-release/update.key`
  (no password, mode 600). Keep a second copy in a password manager.
- **GitHub secret:** `CARDBOARD_UPDATE_KEY` holds the whole secret key file, both lines (the
  `untrusted comment:` line and the key line). The `sign` job writes it to a file, signs, checks
  every signature against `packaging/update.pub`, and deletes the file. Without the secret the job
  fails instead of publishing an unsigned release.

Set it up once (repository Settings → Secrets and variables → Actions → New repository secret), or:

```sh
gh secret set CARDBOARD_UPDATE_KEY --repo goerll/cardboard < ~/.config/cardboard-release/update.key
```

Each signature's trusted comment is `cardboard file:<archive> version:<version>`, and the app
checks both fields, so a valid signature can't be reused for another platform's archive or to push
an older release.

To sign or check something by hand:

```sh
minisign -S -s ~/.config/cardboard-release/update.key -m cardboard-linux-x64.tar.gz \
  -t "cardboard file:cardboard-linux-x64.tar.gz version:0.3.0"
minisign -V -p packaging/update.pub -m cardboard-linux-x64.tar.gz
```

## Rotating the key

Installed apps only trust the key they were built with, so a rotation takes one release to
spread:

1. `minisign -G -W -p new.pub -s new.key` (outside the repo).
2. Release N, still signed with the old key, ships the new public key. Today the app embeds one
   key; for a planned rotation, first change `trusted_key()` in `src/update.rs` to accept both
   (`packaging/update.pub` and `packaging/update-next.pub`) and ship that.
3. Once release N is out and has had time to spread, replace `CARDBOARD_UPDATE_KEY` with the new
   secret, make the new key the only one in `packaging/update.pub`, and release N+1 signed with it.

Apps older than release N will see N+1's signature as invalid and stay put; they update by
re-running the install command, which doesn't use this key (it downloads over HTTPS from GitHub).

## If the secret key is lost or leaks

- **Lost:** nothing can sign updates that today's apps accept. Generate a new pair, put the new
  public key in `packaging/update.pub`, set the new secret, and release. Installed apps won't
  auto-update to it; tell users (release notes, website) to re-run the install command once. After
  that they auto-update normally.
- **Leaked:** treat it as lost, and do it right away. Also remove the leaked secret from GitHub,
  and audit recent releases' assets and signatures. Someone holding the key still needs to get a
  file onto the `goerll/cardboard` release page (the app only downloads from the URLs GitHub's API
  lists for that repository), so also rotate any GitHub tokens that could have leaked with it.
