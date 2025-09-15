# cargo-caps

A Rust tool for auditing and analyzing the capabilities of Rust crates based on the symbols they link to.

## Warning
Only tested on macOS.
Some of the code was AI generated and has not yet been vetted by human eyes.
Half-finished.
Not ready for production.

## Installation
`cargo install --path ./crates/cargo-caps/`

TODO: publish `cargo-caps` on crates.io


## Run it
Make sure you're in the root of a cargo project, then run:

> `cargo-caps build`

This will build your local project, and while doing so, print the capabilities of each crate it depends on, directly or indirectly.

// TODO: show example output

## Test it

Run cargo-caps on self:

```bash
cargo run -p cargo-caps -- build
```


## What is `cargo-caps` for?
Any package manager like `cargo` has a _trust_ issue.
You want to add that nice 3rd party crate, but to do so you must trust it, and all the other transitive dependencies it pulls in.
You can read their source code, but that's a LOT of work, and so you don't.

Enter `cargo-deny`.
`cargo-deny` will verify what a crate is capable and incapble of.

Worried the crate will spawn a thread? It can't, unless it has the `thread` capability.
Worried it will read your files, or mess with your filesystem? It can't without the `fs` capability.
Worried it will communicate with nefarious actors over the inernet? Better check whether or not it has the `net` capability!

The plan is that you should have a `cargo-deny` config file in your repository where you can specify what capabilities each crate you depend on is allowed.
If a crate uses more than it is allowed, you will get a failure when running `cargo-deny check`.

To keep the config file short you can add some base level of capabilities that you always allow.
For instance, you may allow all crates to `panic`, `alloc` memory and tell the `time`, but anything beyond that you must allow-list explicitly.

## How it works
`cargo-caps build` will compile your code and all its dependencies (like `cargo build`) and then analyze the linker symbols.
Based on these symbols `cargo-caps` will then infer _capabilities_ of each library.

For instance: if a crate links with symbols `std::net::` then `cargo-deny` infers that the crate has the capability to communicate over network.

See [`default_rules.ron`](crates/cargo-caps/src/default_rules.ron) for how different symbols are categorized.

Any unknown symbol will lead to the crate being assigned the capability of `any`, which is the conservative and safe thing to do.
TODO: consider splitting out `unknown` and `any`.


## Capabilities
`cargo-caps` currently can distinguish between the following capabilities:
- `alloc` - allocate memory (applied to everything in `std`)
- `panic` - can cause a `panic!` (applied to everything in `std`)
- `time` - measuring time and telling current time
- `sysinfo` - reading environment variables, process info, …
- `stdio` - read/write stdin/stdout/stderr
- `thread` - spawn threads
- `net` - communicate over the network
- `fs` - filesystem access (read and/or write)
- `any` - can do anything

Things that will get a crate put in the `all` bucket includes calling into an opaque library, or starting another process.
Using any symbol not yet categorized in [`default_rules.ron`](crates/cargo-caps/src/default_rules.ron) will also put you in the `all` bucket.


## Limitations
`cargo-deny` can tell whether or not a library can use the network, but not to where it connects.
`cargo-deny` can only tell that a crate is using the file system, but not which files are being read.
If you want this sort of fine-grained capabilities, I suggest you take a look at [cap-std](https://github.com/bytecodealliance/cap-std).

`cargo-deny` also works on a per-crate basis.
Perhaps only a single function in a crate uses the file system, and you aren't calling that.
The crate will still be labeles as using the `fs` capability.

`cargo-deny` is not perfect, but it is better than the status quo.


## Details
### Unix `FILE`:s
`fwrite/fread` can be used on any FILE, including network sockets, BUT `fwrite/fread` is NOT considered a high capability.
Only _opening_ a FILE requires a specific capability (e.g. `net` to open a socket, of `fs` to open a file).
To be able to write to a FILE you first need to open it, OR another crate must hand you that FILE handle, thus handing you the capability.

Similarly with dynamic calls: a crate that calls some callback is not responsible for the capabilities of that callback.


### "Edge crates"
For some crates (hopefully most!), the capabilities is just the union of the capabilities of all its dependent crates.
This means that if we have accurate capabilities for all the crates it depend on, we can confidently assign capabilities to the crate without having to audit it.

However, there will be some _edge_ crates where that won't work, if it uses some 3rd party thing that is opaque to cargo-caps (e.g. dynamically linking with a C library).
In these cases, we must conservatively assign them the `any` capability set (the union of everything).
However, this will infect all dependent crates, so that the whole crate eco system gets labels with `any`.
If a `curl` crate interacts with `libcurl`, we want some way to say "The capability of this crate is actually just `net`".
For this we need _signing_. A trusted person verifies that the crate has a certain set of capabilities, and then signs a specific release and/or commit hash of that crate.

Another example: `ffmpeg-sidecar` is a crate that starts and controls an `ffmpeg` process.
If you trust ffmpeg not to do any shenanigans, then you could sign `ffmpeg-sidecar` to be excluded from `net` and `fs` capbilties.

Currently a Rust developer ha to verify _all_ dependencies it pulls in (even transitive ones!), but with cargo-caps you only need to trust these (hopefully few) _edge_ crates.
