# Vibe

Linux x86_64 and ARM64 (`aarch64-linux`). Requires a PipeWire session.
Captures playback, not the microphone.

## Cargo

Requires Rust and `pw-record` from PipeWire on `PATH`.

```sh
cargo run --release
cargo build --release
./target/release/vibe
```

## Nix

Run without installing:

```sh
nix run git+https://mnemosyne.sigilzero.dev/sigilzero/vibe
```

Install into your user profile:

```sh
nix profile add git+https://mnemosyne.sigilzero.dev/sigilzero/vibe
vibe
```

From a checkout:

```sh
nix run .
nix build
./result/bin/vibe
```

The Nix package includes PipeWire's capture tool.

## Usage

```sh
vibe --scene "Julia"
vibe --list
vibe --demo
vibe --help
```

With Cargo, pass options after `cargo run --release --`; with Nix, after
`nix run git+https://mnemosyne.sigilzero.dev/sigilzero/vibe --`.

Remembers the last visualiser on normal exit. `--scene` overrides it.
State: `$XDG_STATE_HOME/vibe/last-scene`, or
`~/.local/state/vibe/last-scene`.

## Keys

- `←` / `→`: switch visualiser
- `m`: menu; `Esc`: close
- `p`: palettes; `?`: controls
- `+` / `-`: sensitivity
- `d`: demo; `r`: reconnect audio
- `q` / `Ctrl-C`: quit
