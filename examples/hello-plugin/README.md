# hello-plugin

The example plugin, and the fixture `crates/proton-plugin/tests/component.rs`
runs against. It ships a theme and two commands, one of which reads the buffer
through a host call — between them that is every shape in the WIT world.

It is **not** a workspace member. It builds for `wasm32-unknown-unknown` with
its own `[profile]`, and a crate in the workspace cannot have one; keeping it
out also means `cargo test` at the root never tries to build it for the host.

## Rebuilding

The committed `hello.wasm` is what the tests load, so they need no wasm
toolchain. Rebuilding it does:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-tools
cd examples/hello-plugin
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new \
  target/wasm32-unknown-unknown/release/hello_plugin.wasm \
  -o ../../crates/proton-plugin/tests/plugins/hello/hello.wasm
```

`component new` is the step that turns a core module into a component: the
compiler emits one with a custom section describing the WIT world, and this
wraps it in the component the host can actually instantiate.
