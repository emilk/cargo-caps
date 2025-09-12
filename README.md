# Capabara

A Rust tool for extracting and demangling linker symbols for macOS binaries, organized by crate.

## Test it

Run capabara on self:

```bash
./bootstrap.sh
```

## Warning
This is a vibe-coded experiment.

Only tested on macOS.

## Capabilities
Some that come to mind:

- `panic` (anything in `std`, and some of `core`)
- `alloc`
- `time` - measuring time, telling current time
- `sysinfo` - reading environment variables, process info, …
- `stdio`
- `thread` - spawn threads
- `net`
- `fopen` - we don't dinstiguish between read/write since `fopen` makes that hard anyways

We also have `None` for the empty set an `All` for the complete set.

Thing that will get you put in the `All` bucket includes calling into an opaque library, or starting another process.

## Installation
`cargo install --path ./crates/cargo-caps/`

## TODO
- [ ] Read through and clean up the code
- [ ] Fix all TODOs
- [ ] Write docs



## Details
### Unix `FILE`:s
`fwrite/fread` can be used on any FILE, including network sockets, BUT is NOT considered a high capability.
Only _opening_ a FILE requires capability. To be able to write to a FILE you first need to open it, OR another crate must hand you that FILE handle, thus handing you the capability.

Similarly with dynamic calls: a crate that calls some callback is not responsible for the capabilities of that callback.


### Standard capabilities
We start by assigning capabilities to different part of the Rust standard library: `std::net` for networking, `std::fs` for file system access, etc. If a crate uses any part of these, they are marked as having that capability.

We do the same for common sys-calls.


### Signing edge crates
For some crates (hopefull most!), the capabilities is just the union of the capabilities of all its  dependent crates.
This means that if we have accurate capabilities for all the crates it depend on, we can confidently assign capabilities to the crate without having to audit it.

However, there will be some _edge_ crates where that won't work, if it uses some 3rd party thing that is opaque to Capabara (e.g. dynamically linking with a C library).
In these cases, we must conservatively assign them the "All" capability set (the union of everything).
However, this will infect all dependent crates, so that the whole crate eco system gets labels with "All".
If a `curl` crate interacts with `libcurl`, we want some way to say "The capability of this crate is actually just Network".
For this we need _signing_. A trusted person verifies that the crate has a certain set of capabilities, and then signs a specific release and/or commit hash of that crate.

Another example: `ffmpeg-sidecar` is a crate that starts and controls an `ffmpeg` process.
If you trust ffmpeg not to do any shenanigans, then you could sign `ffmpeg-sidecar` to be excluded for `Net` and `FileSystem` capbilties.

Currently a Rust developer ha to verify _all_ dependencies it pulls in (even transitive ones!), but with capabara you only need to trust these (hopefully few) _edge_ crates.
