
## TODO
- Read through and clean up the code
- Fix all TODOs
- Write docs
- Do source-code analysis to flag anything using
  - inline assembly (can be used for sys-calls)
  - unsafe


There are two ways to figure out the dependencies of a crate:
A) ask the linker (bad)
B) ask cargo metadata (good)


### Testing
- add test for build.rs
- add test for proc-macros
