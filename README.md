# Vibe

Fun lil rust CLI music visualiser for linux using ratatui :>

Vibe has 100 visualisers so far, and it fits your terminal colours perfectly!

![Lorentz Attractor](screenshots/4.png)
![Field](screenshots/2.png)
![Chaotic Strings](screenshots/3.png)

## Cargo

```sh
cargo run --release
cargo build --release
./target/release/vibe
```

## Nix

Run without installing:

```sh
nix run git+https://tangled.org/sigilzero.dev/vibe
```

Install into your user profile:

```sh
nix profile add git+https://tangled.org/sigilzero.dev/vibe
vibe
```

## Development

```sh
nix develop
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
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
