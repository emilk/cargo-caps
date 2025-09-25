# cargo-caps

A Rust tool for auditing and analyzing the capabilities of Rust crates based on the symbols they link to.

Say a the popular (and fictitious) crate `super_nice_xml_parser` gets compromised by some evil actor and now mines bitcoins and sends them to some server.
Anyone depending on that crate (directly or indirectly) would be vulnerable.
But running `cargo-caps check` you get an error (on CI or locally),
and in the output you see that the `super_nice_xml_parser` crate suddenly has new capabilities of `thread` and `net`.

In short: `cargo-caps` is a useful tool for auditing 3rd party crates.

## ⚠️ Warning - Experimental!
* Only tested on macOS
* Some of the code was AI generated and has not yet been vetted by human eyes
* Half-finished
* Not ready for production

## Related projects
* [cap-std](https://github.com/bytecodealliance/cap-std) - a capability-based library for things like filesystem and network access
* [cargo-audit-build](https://github.com/tmpfs/cargo-audit-build/) - tool for auditing `build.rs` files
* [cargo-audit](https://crates.io/crates/cargo-audit) - scans dependency tree for RUSTSEC advisories
* [cargo-geiger](https://github.com/geiger-rs/cargo-geiger) - find usage of `unsafe` in crate tree
* [cargo-supply-chain](https://github.com/rust-secure-code/cargo-supply-chain) - who are you trusting?
* [cargo-vet](https://github.com/mozilla/cargo-vet) - can be used to mark crates as "vetted" (audited)

## Design goals
* Fail safe: if `cargo-caps` can't understand it then assume the worst
* Minimize noise: if we bother the user too much, they'll get desensitized

## Installation
`cargo install cargo-caps --locked`

## Run it
Make sure you're in the root of a cargo project, then run:

> `cargo-caps init`

This creates a `cargo-caps.eon` file where you can specify what crates are allowed what capabilities.
Next run:

> `cargo-caps check`

This will build your local project, and while doing so, print the capabilities of each crate it depends on, directly or indirectly.

## What is `cargo-caps` for?
Any package manager like `cargo` is vulnerable to supply chain attacks.
You want to add that nice 3rd party crate, but to do so you must trust it, and all the other transitive dependencies it pulls in.
And you must also trust future update whenever you run `cargo update`.

You can read their source code, but that's a LOT of work, and so you don't.

Enter `cargo-deny`.
`cargo-deny` will verify what a crate is capable and incapble of.

Worried a create will read your files, or mess with your filesystem? It can't without the `fs` capability.
Worried it will communicate with nefarious actors over the inernet? Better check whether or not it has the `net` capability!

You can configure exactly what each dependency is allowed in a `cargo-deny.eon` config file in your repository.
You can then run `cargo-deny check` to check if any dependency does more than it is allowed to.

## How it works
### Symbol analyzer
`cargo-caps check` will run `cargo build` and then analyze the linker symbols of each built library.
Based on these symbols `cargo-caps` will then infer _capabilities_ of each library.

For instance: if a crate links with symbols in `std::net::` then `cargo-deny` infers that the crate has the capability to communicate over network.

See [`default_rules.ron`](crates/cargo-caps/src/default_rules.ron) for how different symbols are categorized.

Any unknown symbol will lead to the crate being assigned the capability of `any` (fail-safe).

TODO: consider splitting out an `unknown` capability from `any`.

### Source analyzer
A lot of crates have `build.rs` files that have the possibility to do anything.
But `build.rs` files gets compiled to binaries, making analyzing their symbols a lot harder (they just pull in a lot more by default).
Therefore `cargo-caps` will instead parse `build.rs` files (using [`syn`](crates.io/crates/syn)).
The source analyzer works only for simple `build.rs` files (which are most of them).
For more complex things, you the user will have to manually audit (or trust).
To help, `cargo-caps` will print the path to the source code so you can more easily find and read the code.

## TODO:
* Do source code analysis to find and flag all `unsafe`
  * (with `unsafe` a crate can potentially do any evil sys-call)

## Capabilities
`cargo-caps` currently can distinguish between the following capabilities:
- `alloc` - allocate memory (applied to everything in `std`)
- `panic` - can cause a `panic!` (applied to everything in `std`)
- `time` - measuring time and telling the current time
- `sysinfo` - reading environment variables, process info, …
- `stdio` - read/write stdin/stdout/stderr
- `thread` - spawn threads
- `net` - communicate over the network
- `fs` - filesystem access (read and/or write)
- `any` - can do anything

Things that will get a crate put in the `any` bucket includes calling into an opaque library, or starting another process.
Using any symbol not yet categorized in [`default_rules.ron`](crates/cargo-caps/src/default_rules.ron) will also put you in the `any` bucket.


## Limitations
`cargo-deny` can tell whether or not a library can use the network, but not to where it connects.
`cargo-deny` can only tell that a crate is using the file system, but not which files are being read.
If you want this sort of fine-grained capabilities, I suggest you take a look at [cap-std](https://github.com/bytecodealliance/cap-std).

`cargo-deny` also works on a per-crate basis.
Perhaps only a single function in a crate uses the file system, and you aren't calling that.
The crate will still be labeles as using the `fs` capability.

Macros produce can make `cargo-deny` point the finger at the wrong crate.
For instance, if a `simple_log!` macro actually spawns a thread and starts a network connection, then the `net` capability will show up for the crate _calling_ `simple_log!`, as opposed to the crate `defining` it.

`cargo-deny` is not perfect, but it can still be a useful layer of defence.


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

## Developing

Run cargo-caps on self:

```bash
cargo run --release -- check
```
