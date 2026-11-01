# Vibe

Fun lil rust CLI music visualiser for linux using ratatui :>

## Cargo

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


## Usage

```sh
vibe --scene "Julia"
vibe --list
vibe --demo
vibe --help
```

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
