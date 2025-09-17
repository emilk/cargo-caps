
## TODO
- Read through and clean up the code
- Fix all TODOs
- Write docs
- publish on crates.io
- publish binaries for `binstall`


### Testing
- add test for build.rs
- add test for proc-macros


### Config file
- what format? toml? eon?
- Make having build-dependencies (e.g. build.rs) an opt-in capability

```
{
    // Capabilities all crates are allowed:
    crates: "*"
    caps: [
        "alloc"
        "panic"
        "time"
    ]
}
{
    // These crates are allowed to have build.rs files
    // TODO: how do we enforce this?
    // TODO: it would be nice to be able to specify the capabilities of build.rs files
    caps: ["build_rs"]
    crate: ["foo", "…"]
}
{
    // These crates are allowed to access the network:
    caps: ["net"]
    crates: [
        "ureq"
        "tonic"
        "tonic/*" anything tonic depends on
    ]
}
```
