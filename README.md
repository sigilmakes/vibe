# Vibe

Requires a Linux PipeWire session. Captures playback, not the microphone.

## Run

```sh
nix run .
nix run . -- --scene "Julia"
nix run . -- --list
nix run . -- --demo
nix run . -- --help
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

## Build

```sh
nix build
./result/bin/vibe
```
