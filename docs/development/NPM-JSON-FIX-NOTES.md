# zSoft Manager v0.4.3 – npm global outdated JSON hotfix

## Bug
`npm outdated --global --depth=0 --json --silent` returns JSON, but the Bash fallback
`"${result:-{}}"` silently appends `}`. The Rust JSON parser then fails with
`trailing characters` even for normal npm output.

## Fix
Use an explicit empty-output condition and print the exact npm stdout unchanged.
One offline Rust regression test runs the actual GLOBAL_OUTDATED Bash script with
mock NVM/npm functions for an outdated-package result (npm exit 1) and an empty result.

## Apply
Extract into the existing zSoft Manager project root, overwriting only `src/node.rs`.
Do not replace `src/update.rs`, which may contain the separate E0505 compiler fix.

## Test
`cargo fmt`
`cargo test`
`cargo build --release`
`cargo run --release`

## Changelog entry
Under `## [0.4.3]` add a `### Fixed` section:
`- Fixed global npm outdated JSON parsing: Bash fallback added a spurious closing brace to the response.`

This does not require a new release unless v0.4.3 has already been published.
