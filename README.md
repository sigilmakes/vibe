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
vibe --list-audio
vibe --demo
vibe --help
```

## Audio

By default Vibe captures the output monitor of the default sink. It never
captures the microphone.

```sh
vibe --list-audio          # sinks, sources, serials and defaults
vibe --check-audio 3       # sample for three seconds and report the source
vibe --audio-target auto_null
```

`--audio-source` accepts `auto`, `pipewire`, `pulse`, `command`, or `file`.
`auto` prefers `pw-record` and falls back to `parec`. For any other backend
that writes raw PCM to stdout:

```sh
vibe --audio-source command \
  --audio-command 'parec --format=float32le --rate=48000 --channels=2 --device=@DEFAULT_MONITOR@'
vibe --audio-source file --audio-file track.raw
```

Raw PCM defaults to `f32le`, 48 kHz, stereo. Override it with
`--audio-format s16le|s32le`, `--audio-rate`, and `--audio-channels`.

If `--check-audio` reports "connected but silent", the selected sink is live
but has no signal. A dummy sink such as `auto_null` is a common cause.

## Display

```sh
vibe --fps 30
vibe --color-depth ansi256
vibe --glyphs ascii
vibe --follow-theme
```

`--color-depth auto` respects `NO_COLOR` and prefers truecolour when
`COLORTERM` advertises it. `--glyphs auto` falls back to ASCII for `TERM=dumb`
or a C locale. Terminal colours are queried once at startup by default;
`--follow-theme` re-queries them every second.

## State

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
