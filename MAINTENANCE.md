This document explains how to perform the project's maintenance tasks.

### Creating a new release

#### Artifacts

* a tag of the version number
* an updated `CHANGELOG.md` with the latest version
* a new [crate version](https://crates.io/crates/flate2/versions)

#### Process

To generated all the artifacts above, one proceeds as follows:

1. `git checkout -b release-<next-version>` - move to a branch to prepare making changes to the repository. *Changes cannot be made to `main` as it is protected.*
2. Edit `Cargo.toml` to the next package version.
3. [`cargo changelog --write`](https://crates.io/crates/cargo-smart-release) to update the `CHANGELOG.md` file with all changes from the git history. Note that it extracts only.
   [conventional commit messages](https://keepachangelog.com/en/1.0.0/). 
   *Adjust the version headline to the `<next-version>` from `Unreleased`*.
4. Commit all changes.
5. `gh pr create` to create a new PR for the current branch and **get it merged**.
6. `cargo publish` to create a new release on `crates.io`.
7. `git tag <next-version>` to remember the commit.
8. `git push --tags` to push the new tag.
9. Go to the newly created release page on GitHub and edit it by pressing the "Generate Release Notes" and the `@` button. Save the release.

Note that in this workflow, the changelog for the current release maybe a bit bare if *conventional commit messages* were not used, but
there is still the auto-generated commit-list of everything that went into the release which is useful for folks who don't think of GitHub releases
or want a file in their crate sources.
