use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

const ORIGINAL_SCENES: &[&str] = &[
    "Spectral Flower",
    "Waveform Scope",
    "Liquid Topography",
    "Electric Aurora",
    "Orbital Bloom",
    "Lissajous Ribbons",
    "Swarm",
    "Type Percussion",
    "Harmonic Lace",
    "Wire Sculpture",
    "Block Composition",
    "Murmuration",
    "Fireflies",
    "Spectrum Bars",
    "Spectrum Waterfall",
    "Mandelbrot",
    "Julia",
    "Attractor Storm",
    "Reaction Garden",
];

/// The renderer family that owns a contiguous run of global scene indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Family {
    Original,
    Glyphs,
    Sculptures,
    Flocks,
    Spectrum,
    Fractals,
    Chaos,
    Curves,
    Waves,
    Cells,
    Geometry,
    Particles,
    Typography,
    Organic,
    FractalMath,
    Circuit,
    Metaballs,
    Fluid,
    Tunnel,
    Kaleidoscope,
    Weather,
    Terrain,
    Textile,
    Orbital,
    City,
}

/// Tag vocabulary shared by every scene. Filters match against these.
pub const TAGS: &[&str] = &[
    "calm",
    "dense",
    "minimal",
    "organic",
    "geometric",
    "particles",
    "fractal",
    "chaotic",
    "flow",
    "structure",
    "bright",
    "dark",
];

/// Tags for a scene, chosen per family with a few local special cases for the
/// original scenes so the vocabulary stays descriptive without per-scene data.
const fn scene_tags(family: Family, local: usize) -> &'static [&'static str] {
    match family {
        Family::Original => match local {
            0 => &["calm", "organic", "bright"],
            1 => &["structure", "flow", "minimal"],
            2 => &["organic", "flow", "calm"],
            3 => &["bright", "flow", "chaotic"],
            4 => &["geometric", "structure", "calm"],
            5 => &["flow", "minimal", "structure"],
            _ => &["particles", "chaotic", "organic"],
        },
        Family::Glyphs => &["minimal", "structure", "geometric"],
        Family::Sculptures => &["structure", "geometric", "minimal"],
        Family::Flocks => &["particles", "flow", "organic"],
        Family::Spectrum => &["structure", "bright", "flow"],
        Family::Fractals => &["fractal", "dense", "structure"],
        Family::Chaos => &["chaotic", "flow", "dark"],
        Family::Curves => &["flow", "minimal", "calm"],
        Family::Waves => &["flow", "calm", "structure"],
        Family::Cells => &["structure", "dense", "organic"],
        Family::Geometry => &["geometric", "structure", "minimal"],
        Family::Particles => &["particles", "flow", "chaotic"],
        Family::Typography => &["structure", "minimal", "geometric"],
        Family::Organic => &["organic", "flow", "calm"],
        Family::FractalMath => &["fractal", "dense", "structure"],
        Family::Circuit => &["structure", "geometric", "bright"],
        Family::Metaballs => &["organic", "flow", "calm"],
        Family::Fluid => &["flow", "organic", "calm"],
        Family::Tunnel => &["geometric", "flow", "dark"],
        Family::Kaleidoscope => &["geometric", "bright", "structure"],
        Family::Weather => &["chaotic", "flow", "dark"],
        Family::Terrain => &["structure", "organic", "calm"],
        Family::Textile => &["structure", "geometric", "minimal"],
        Family::Orbital => &["geometric", "calm", "dark"],
        Family::City => &["structure", "geometric", "dense"],
    }
}

/// Original scenes first, grouped by the renderer that owns them.
const ORIGINAL_FAMILIES: [(Family, usize); 7] = [
    (Family::Original, 7),
    (Family::Glyphs, 2),
    (Family::Sculptures, 2),
    (Family::Flocks, 2),
    (Family::Spectrum, 2),
    (Family::Fractals, 2),
    (Family::Chaos, 2),
];

const NEW_FAMILIES: [(Family, &[&str]); 18] = [
    (Family::Curves, crate::families::curves::NAMES),
    (Family::Waves, crate::families::waves::NAMES),
    (Family::Cells, crate::families::cells::NAMES),
    (Family::Geometry, crate::families::geometry::NAMES),
    (Family::Particles, crate::families::particles::NAMES),
    (Family::Typography, crate::families::typography::NAMES),
    (Family::Organic, crate::families::organic::NAMES),
    (Family::FractalMath, crate::families::fractalmath::NAMES),
    (Family::Circuit, crate::families::circuit::NAMES),
    (Family::Metaballs, crate::families::metaballs::NAMES),
    (Family::Fluid, crate::families::fluid::NAMES),
    (Family::Tunnel, crate::families::tunnel::NAMES),
    (Family::Kaleidoscope, crate::families::kaleidoscope::NAMES),
    (Family::Weather, crate::families::weather::NAMES),
    (Family::Terrain, crate::families::terrain::NAMES),
    (Family::Textile, crate::families::textile::NAMES),
    (Family::Orbital, crate::families::orbital::NAMES),
    (Family::City, crate::families::city::NAMES),
];

const SCENE_COUNT: usize = 200;

#[derive(Clone, Copy)]
struct SceneEntry {
    name: &'static str,
    family: Family,
    local: usize,
    tags: &'static [&'static str],
}

const REGISTRY: [SceneEntry; SCENE_COUNT] = build_registry();

const fn build_registry() -> [SceneEntry; SCENE_COUNT] {
    let mut scenes = [SceneEntry {
        name: "",
        family: Family::Original,
        local: 0,
        tags: &[],
    }; SCENE_COUNT];
    let mut index = 0;
    let mut family = 0;
    while family < ORIGINAL_FAMILIES.len() {
        let (kind, count) = ORIGINAL_FAMILIES[family];
        let mut local = 0;
        while local < count {
            scenes[index] = SceneEntry {
                name: ORIGINAL_SCENES[index],
                family: kind,
                local,
                tags: scene_tags(kind, local),
            };
            index += 1;
            local += 1;
        }
        family += 1;
    }
    let mut family = 0;
    while family < NEW_FAMILIES.len() {
        let (kind, names) = NEW_FAMILIES[family];
        let mut local = 0;
        while local < names.len() {
            scenes[index] = SceneEntry {
                name: names[local],
                family: kind,
                local,
                tags: scene_tags(kind, local),
            };
            index += 1;
            local += 1;
        }
        family += 1;
    }
    assert!(index == SCENE_COUNT);
    scenes
}

pub const SCENES: &[&str] = &catalog();

const fn catalog() -> [&'static str; SCENE_COUNT] {
    let mut names = [""; SCENE_COUNT];
    let mut index = 0;
    while index < SCENE_COUNT {
        names[index] = REGISTRY[index].name;
        index += 1;
    }
    names
}

/// Tags assigned to a scene, in `TAGS` order of appearance.
pub fn tags(scene: usize) -> &'static [&'static str] {
    assert!(scene < SCENE_COUNT);
    REGISTRY[scene].tags
}

const fn scene_route(scene: usize) -> (Family, usize) {
    assert!(scene < SCENE_COUNT);
    let entry = REGISTRY[scene];
    (entry.family, entry.local)
}

/// Normalized live audio for one rendered frame.
///
/// Continuous envelopes (`energy`, `bass`, `mid`, `treble`, `onset`, `kick`,
/// `beat`, `novelty`, `transients`) and `waveform` are scaled by the
/// sensitivity setting. Distribution and position features (`chroma`,
/// `centroid`, `flatness`, `width`, `balance`) and `beat_phase` are not.
///
/// Envelopes and spectrum come from a mono mix; `width` and `balance` come
/// from the first two input channels before mixing. `transients` carries
/// independent low, low-mid, presence, and air attacks; `onset` is their
/// maximum.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reactivity {
    pub energy: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub onset: f32,
    pub kick: f32,
    pub beat: f32,
    /// Fraction of the current beat in [0, 1). Only advances while the tempo
    /// tracker is locked; use `locked_beat_phase` or `tempo_confidence` for
    /// beat-synced motion.
    pub beat_phase: f32,
    /// Confidence of the tempo estimate in [0, 1]. Treat `beat_phase` as
    /// meaningful only when this is clearly above zero.
    pub tempo_confidence: f32,
    pub waveform: [f32; 32],
    pub transients: [f32; 4],
    pub novelty: f32,
    pub chroma: [f32; 12],
    pub centroid: f32,
    pub flatness: f32,
    /// Side/mid energy ratio in [0, 1]: 0 mono, ~0.5 uncorrelated stereo.
    pub width: f32,
    /// Right-minus-left loudness balance in [-1, 1]; zero when centred.
    pub balance: f32,
}

impl Reactivity {
    /// Beat phase, or zero when the tempo estimate is too weak to trust.
    /// `beat_phase` stops advancing while the tracker is unlocked, so scenes
    /// must not treat it as a free-running clock.
    pub fn locked_beat_phase(&self) -> f32 {
        if self.tempo_confidence >= 0.25 {
            self.beat_phase
        } else {
            0.0
        }
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

fn line_glyph(tangent: [f32; 2]) -> char {
    let slope = tangent[1] / (tangent[0] + 0.00001);
    if slope.abs() < 0.4 {
        '─'
    } else if slope.abs() > 2.5 {
        '│'
    } else if slope > 0.0 {
        '╲'
    } else {
        '╱'
    }
}

fn distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let u = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
        / (d[0] * d[0] + d[1] * d[1]).max(0.00001))
    .clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * u).hypot(p[1] - a[1] - d[1] * u)
}

struct Pixel {
    color: Color,
    mark: Option<char>,
}

#[derive(Clone, Copy)]
struct Impact {
    age: f32,
    strength: f32,
    normal: [f32; 2],
    channel: usize,
}

/// Motion is integrated, so changing an envelope never jumps a time multiplier.
/// Short-lived impacts preserve rapid attacks after their source envelope falls.
#[derive(Default)]
pub struct Renderer {
    curves: Option<crate::families::curves::Renderer>,
    waves: Option<crate::families::waves::Renderer>,
    cells: Option<crate::families::cells::Renderer>,
    geometry: Option<crate::families::geometry::Renderer>,
    particles: Option<crate::families::particles::Renderer>,
    typography: Option<crate::families::typography::Renderer>,
    organic: Option<crate::families::organic::Renderer>,
    fractalmath: Option<crate::families::fractalmath::Renderer>,
    circuit: Option<crate::families::circuit::Renderer>,
    metaballs: Option<crate::families::metaballs::Renderer>,
    fluid: Option<crate::families::fluid::Renderer>,
    tunnel: Option<crate::families::tunnel::Renderer>,
    kaleidoscope: Option<crate::families::kaleidoscope::Renderer>,
    weather: Option<crate::families::weather::Renderer>,
    terrain: Option<crate::families::terrain::Renderer>,
    textile: Option<crate::families::textile::Renderer>,
    orbital: Option<crate::families::orbital::Renderer>,
    city: Option<crate::families::city::Renderer>,
    glyphs: Option<crate::families::glyphs::Renderer>,
    sculptures: Option<crate::families::sculptures::Renderer>,
    flocks: Option<crate::families::flocks::Renderer>,
    spectrum: Option<crate::families::spectrum::Renderer>,
    fractals: Option<crate::families::fractals::Renderer>,
    chaos: Option<crate::families::chaos::Renderer>,
    last_time: Option<f32>,
    rotation: f32,
    expansion: f32,
    expansion_velocity: f32,
    attacks: [f32; 4],
    cooldown: [f32; 4],
    serial: i32,
    impacts: Vec<Impact>,
}

impl Renderer {
    fn advance(&mut self, time: f32, audio: &Reactivity) {
        let dt = self
            .last_time
            .map_or(1.0 / 60.0, |last| (time - last).clamp(0.0, 0.1));
        self.last_time = Some(time);
        self.rotation += dt * (0.012 + audio.mid * 0.13);
        for impact in &mut self.impacts {
            impact.age += dt;
        }
        self.impacts.retain(|impact| impact.age < 0.18);
        for channel in 0..4 {
            self.cooldown[channel] = (self.cooldown[channel] - dt).max(0.0);
            // Each channel owns one attack band. Broad signals such as `onset`
            // and `novelty` must not retrigger every channel for one hit.
            let attack = audio.transients[channel].clamp(0.0, 1.0);
            let rise = attack - self.attacks[channel];
            if rise > 0.09 && attack > 0.16 && self.cooldown[channel] == 0.0 {
                self.serial = self.serial.wrapping_add(1);
                let seed = hash(self.serial, channel as i32);
                let angle = seed * TAU;
                if self.impacts.len() == 32 {
                    self.impacts.remove(0);
                }
                self.impacts.push(Impact {
                    age: 0.0,
                    strength: attack,
                    normal: [angle.cos(), angle.sin()],
                    channel,
                });
                if channel == 0 {
                    self.expansion_velocity += attack * 2.7;
                }
                self.cooldown[channel] = 0.035;
            }
            self.attacks[channel] = attack;
        }
        // A damped spring supplies expansion followed by an angular recoil.
        // Substeps keep this stable when a terminal frame takes longer than usual.
        let steps = (dt / 0.008).ceil().max(1.0) as usize;
        let step = dt / steps as f32;
        for _ in 0..steps {
            self.expansion_velocity +=
                (-900.0 * self.expansion - 36.0 * self.expansion_velocity) * step;
            self.expansion += self.expansion_velocity * step;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        scene: usize,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &crate::palette::Theme,
    ) {
        self.advance(time, audio);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (family, local) = scene_route(scene);
        match family {
            Family::Original => {
                self.draw_original(buf, area, time, local, palette, bands, audio, theme)
            }
            Family::Glyphs => self
                .glyphs
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Sculptures => self
                .sculptures
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Flocks => self
                .flocks
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Spectrum => self
                .spectrum
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Fractals => self
                .fractals
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Chaos => self
                .chaos
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Curves => self
                .curves
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Waves => self
                .waves
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Cells => self
                .cells
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Geometry => self
                .geometry
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Particles => self
                .particles
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Typography => self
                .typography
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Organic => self
                .organic
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::FractalMath => self
                .fractalmath
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Circuit => self
                .circuit
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Metaballs => self
                .metaballs
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Fluid => self
                .fluid
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Tunnel => self
                .tunnel
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Kaleidoscope => self
                .kaleidoscope
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Weather => self
                .weather
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Terrain => self
                .terrain
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Textile => self
                .textile
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::Orbital => self
                .orbital
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
            Family::City => self
                .city
                .get_or_insert_with(Default::default)
                .draw(buf, area, time, local, palette, bands, audio, theme),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_original(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        scene: usize,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &crate::palette::Theme,
    ) {
        if scene == 6 {
            self.swarm(buf, area, time, palette, bands, audio, theme);
            return;
        }

        // Seven recognizable scalloped petals, drawn as hard facets. Each
        // frequency region and recent attack can displace an individual petal.
        let vertices: [[f32; 2]; 42] = std::array::from_fn(|i| {
            let angle = i as f32 / 42.0 * TAU;
            let petal = i / 6;
            let mut radius = 0.40
                + (angle * 7.0).cos() * 0.105
                + self.expansion
                + audio.bass * 0.055
                + audio.beat * 0.025
                + (angle * 7.0 - audio.locked_beat_phase() * TAU).cos() * audio.beat * 0.008
                + bands[petal * 4].clamp(0.0, 1.0) * 0.10
                + if i % 2 == 0 { 0.055 } else { -0.045 }
                + bands[i * 31 / 41].clamp(0.0, 1.0) * 0.08;
            for impact in &self.impacts {
                let alignment = (angle.cos() * impact.normal[0] + angle.sin() * impact.normal[1])
                    .max(0.0)
                    .powi(6);
                radius += alignment
                    * impact.strength
                    * (1.0 - impact.age / 0.12).max(0.0)
                    * if impact.channel < 2 {
                        0.12
                    } else {
                        if i % 2 == 0 {
                            0.09
                        } else {
                            -0.04
                        }
                    };
            }
            let angle = angle + self.rotation;
            [angle.cos() * radius, angle.sin() * radius]
        });
        // Original two 2:3 parametric paths, sampled once per frame.
        // Attacks displace their shape; they never multiply the animation clock.
        let ribbons = std::array::from_fn(|side| {
            if scene != 5 {
                return [[0.0; 2]; 33];
            }
            std::array::from_fn(|i| {
                let side = side as f32 * 2.0 - 1.0;
                let u = i as f32 / 32.0 * TAU;
                let position = i as f32 / 32.0 * 31.0;
                let index = position.floor() as usize;
                let spectrum = (bands[index]
                    + (bands[(index + 1).min(31)] - bands[index]) * position.fract())
                .clamp(0.0, 1.0);
                let mut displacement = [0.0, 0.0];
                for impact in &self.impacts {
                    let strength = impact.strength * (1.0 - impact.age / 0.18).max(0.0);
                    let frequency = impact.channel as f32 + 1.0;
                    displacement[0] += (u * frequency + side * 0.7).sin() * strength * 0.10;
                    displacement[1] += (u * (frequency + 1.0) - side * 0.4).cos() * strength * 0.10;
                }
                [
                    (u * 2.0 + time * 0.22 + side * (0.22 + audio.mid * 0.3)).sin()
                        * (0.75 + audio.bass * 0.22)
                        + displacement[0],
                    (u * 3.0 - time * 0.17 + side * 0.35).sin()
                        * (0.34 + audio.mid * 0.16 + spectrum * 0.12)
                        + side * audio.onset.clamp(0.0, 1.0) * 0.07
                        + displacement[1],
                ]
            })
        });
        let visible = area.intersection(buf.area);
        let scale = 2.0 / area.height as f32;
        for row in visible.y..visible.bottom() {
            for col in visible.x..visible.right() {
                let x =
                    ((col - area.x) as f32 + 0.5 - area.width as f32 * 0.5) / area.height as f32;
                let y = ((row - area.y) as f32 + 0.25 - area.height as f32 * 0.5) * scale;
                let top = self.pixel(
                    [x, y],
                    &vertices,
                    &ribbons,
                    scene,
                    palette,
                    audio,
                    theme,
                    scale,
                    area.width as f32 / area.height as f32 * 0.5,
                    bands,
                );
                let bottom = self.pixel(
                    [x, y + scale * 0.5],
                    &vertices,
                    &ribbons,
                    scene,
                    palette,
                    audio,
                    theme,
                    scale,
                    area.width as f32 / area.height as f32 * 0.5,
                    bands,
                );
                let cell = &mut buf[(col, row)];
                cell.reset();
                if let Some(mark) = top.mark.or(bottom.mark) {
                    let color = if top.mark.is_some() {
                        top.color
                    } else {
                        bottom.color
                    };
                    cell.set_char(mark)
                        .set_fg(color)
                        .set_bg(theme.sample(palette, 0.0, 0.035));
                } else {
                    cell.set_char('▀').set_fg(top.color).set_bg(bottom.color);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn swarm(
        &self,
        buf: &mut Buffer,
        area: Rect,
        time: f32,
        palette: usize,
        bands: &[f32; 32],
        audio: &Reactivity,
        theme: &crate::palette::Theme,
    ) {
        let visible = area.intersection(buf.area);
        let background = theme.sample(palette, 0.0, 0.035);
        for row in visible.y..visible.bottom() {
            for col in visible.x..visible.right() {
                buf[(col, row)].reset();
                buf[(col, row)].set_char(' ').set_bg(background);
            }
        }
        // The original constellation's seeded lattice and slow diagonal drift.
        // Rasterize its short tails and heads directly, not as a glowing field.
        let seed_at = |x: f32, y: f32| ((x * 127.1 + y * 311.7).sin() * 43_758.547).fract().abs();
        let spacing = 0.36;
        let drift = [time * 0.025, -time * 0.018];
        let half_width = area.width as f32 / area.height as f32 * 0.5;
        let low_attack = audio.transients[0].max(audio.kick);
        let high_attack = audio.transients[2].max(audio.transients[3]);
        let mut particles = Vec::new();
        for cy in ((-1.0 + drift[1]) / spacing).floor() as i32 - 1
            ..=((1.0 + drift[1]) / spacing).ceil() as i32 + 1
        {
            for cx in ((-half_width + drift[0]) / spacing).floor() as i32 - 1
                ..=((half_width + drift[0]) / spacing).ceil() as i32 + 1
            {
                let seed = seed_at(cx as f32, cy as f32);
                if seed < 0.48 {
                    continue;
                }
                let position = seed * 31.0;
                let i = position.floor() as usize;
                let spectrum = (bands[i] + (bands[(i + 1).min(31)] - bands[i]) * position.fract())
                    .clamp(0.0, 1.0);
                let point = [
                    (cx as f32 + 0.2 + seed * 0.6) * spacing
                        + (time * 0.3 + seed * TAU).sin()
                            * (0.025 + audio.bass * 0.09 + low_attack * 0.06)
                        - drift[0],
                    (cy as f32 + 0.2 + seed_at(cy as f32, cx as f32 + 3.0) * 0.6) * spacing
                        + (time * 0.24 + seed * 9.0).cos()
                            * (0.02 + audio.mid * 0.07 + high_attack * 0.06)
                        - drift[1],
                ];
                let tail = [
                    point[0] - 0.025 - spectrum * 0.12 - audio.onset * 0.12 - low_attack * 0.08,
                    point[1] + 0.018 + audio.treble * 0.055 + high_attack * 0.035,
                ];
                particles.push((point, tail, seed, spectrum));
            }
        }
        let cell_position = |point: [f32; 2]| {
            [
                area.x as f32 + point[0] * area.height as f32 + area.width as f32 * 0.5 - 0.5,
                area.y as f32 + point[1] * area.height as f32 * 0.5 + area.height as f32 * 0.5
                    - 0.5,
            ]
        };
        let mut plot = |point: [f32; 2], mark: char, color: Color| {
            let x = point[0].round() as i32;
            let y = point[1].round() as i32;
            if x >= visible.x as i32
                && x < visible.right() as i32
                && y >= visible.y as i32
                && y < visible.bottom() as i32
            {
                buf[(x as u16, y as u16)].set_char(mark).set_fg(color);
            }
        };
        for &(point, tail, seed, _) in &particles {
            let head = cell_position(point);
            let tail = cell_position(tail);
            let tangent = [head[0] - tail[0], head[1] - tail[1]];
            let steps = tangent[0].abs().max(tangent[1].abs()).ceil().max(1.0) as usize;
            for i in 0..steps {
                let u = i as f32 / steps as f32;
                plot(
                    [tail[0] + tangent[0] * u, tail[1] + tangent[1] * u],
                    line_glyph(tangent),
                    theme.sample(palette, seed + time * 0.01, 0.18 + u * 0.35),
                );
            }
        }
        for (point, _, seed, spectrum) in particles {
            plot(
                cell_position(point),
                if high_attack > 0.45 {
                    '*'
                } else if spectrum > 0.35 {
                    '●'
                } else {
                    '•'
                },
                theme.sample(palette, seed + time * 0.01, 0.7 + spectrum * 0.3),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pixel(
        &self,
        p: [f32; 2],
        vertices: &[[f32; 2]; 42],
        ribbons: &[[[f32; 2]; 33]; 2],
        scene: usize,
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
        half_width: f32,
        bands: &[f32; 32],
    ) -> Pixel {
        match scene {
            0 => self.flower(p, vertices, palette, audio, theme, scale),
            1 => self.scope(p, palette, audio, theme, scale, half_width),
            5 => self.ribbons(p, ribbons, palette, audio, theme),
            _ => self.restored_soft(p, scene, palette, audio, bands, theme),
        }
    }

    fn ribbons(
        &self,
        p: [f32; 2],
        ribbons: &[[[f32; 2]; 33]; 2],
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
    ) -> Pixel {
        let mut nearest = f32::MAX;
        let mut phase = 0.0;
        for (side, points) in ribbons.iter().enumerate() {
            for (i, segment) in points.windows(2).enumerate() {
                let a = segment[0];
                let b = segment[1];
                let d = [b[0] - a[0], b[1] - a[1]];
                let u = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]).max(0.00001))
                .clamp(0.0, 1.0);
                let dx = p[0] - a[0] - d[0] * u;
                let dy = p[1] - a[1] - d[1] * u;
                let squared = dx * dx + dy * dy;
                if squared < nearest {
                    nearest = squared;
                    phase = (i + 1) as f32 / 32.0 + side as f32 * 0.28;
                }
            }
        }
        let width = 0.012 + audio.treble * 0.012;
        let line = 1.0 / (1.0 + nearest / (width * width));
        let base = theme.rgb(
            palette,
            phase + self.last_time.unwrap_or(0.0) * 0.025,
            0.008 + line * 0.82,
        );
        let rgb: [u8; 3] = std::array::from_fn(|i| {
            (base[i] as f32 + (theme.foreground[i] as f32 - base[i] as f32) * line.powi(3) * 0.35)
                .round() as u8
        });
        Pixel {
            color: Color::Rgb(rgb[0], rgb[1], rgb[2]),
            mark: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn restored_soft(
        &self,
        [x, y]: [f32; 2],
        scene: usize,
        palette: usize,
        audio: &Reactivity,
        bands: &[f32; 32],
        theme: &crate::palette::Theme,
    ) -> Pixel {
        let t = self.last_time.unwrap_or(0.0);
        let e = audio.energy;
        let bass = audio.bass;
        let mid = audio.mid;
        let treble = audio.treble;
        let onset = audio.onset;
        let low_attack = audio.transients[0].max(audio.kick);
        let high_attack = audio.transients[2].max(audio.transients[3]);
        let glow = |distance: f32, width: f32| {
            let d = distance / width;
            1.0 / (1.0 + d * d)
        };
        // These are the original contour and filament equations, not the broad
        // Gaussian washes. Fast attacks deform them without changing their clock.
        let (phase, light, highlight) = if scene == 2 {
            let u = x
                + (0.24 + bass * 0.15) * (y * 2.4 + t * 0.25).sin()
                + low_attack * 0.10 * (y * 5.0).sin();
            let v = y
                + (0.23 + mid * 0.13) * (x * 2.0 - t * 0.2).sin()
                + onset * 0.06
                + high_attack * 0.065 * (x * 7.0).sin();
            let height = (u * 2.5 + t * 0.17).sin() * 0.55
                + (v * 3.3 - t * 0.23).cos() * 0.35
                + (u * 4.0 + v * 2.0 + t * 0.2).sin() * 0.18;
            let contours = glow(
                (height * 16.0 + t * 0.25).sin(),
                0.08 + e * 0.05 + treble * 0.025,
            );
            let broad = (height * 2.0 + 1.0).sin() * 0.5 + 0.5;
            (
                height * 0.6 + t * 0.018,
                0.12 + broad * 0.24 + contours * 0.62,
                contours * 0.25,
            )
        } else if scene == 3 {
            // Panning moves the curtain sideways; width opens its ribbon.
            let x = x + audio.balance * 0.06;
            let sway = (x * 2.2 + t * 0.25).sin() * 0.28 + (x * 4.7 - t * 0.32).sin() * 0.1;
            let position = (x * 0.22 + 0.5).clamp(0.0, 1.0) * 31.0;
            let i = position.floor() as usize;
            let frequency = bands[i] + (bands[(i + 1).min(31)] - bands[i]) * position.fract();
            let curtain = y
                + sway * (1.0 + mid * 0.6)
                + 0.12
                + frequency * 0.28
                + bass * 0.12
                + onset * 0.05
                + low_attack * 0.10
                + high_attack * 0.045 * (x * 13.0).sin();
            let ribbon = glow(curtain, 0.12 + e * 0.08 + audio.width * 0.05);
            let silk = (x * 27.0 + (y * 3.0 + t * 0.4).sin() * 3.0 + t * 0.5).sin() * 0.5 + 0.5;
            let haze = glow(curtain - 0.25, 0.45) * 0.18;
            (
                x * 0.13 + y * 0.22 + silk * 0.08 + t * 0.025,
                haze + ribbon * (0.35 + silk * (0.6 + treble * 0.15)),
                ribbon.powi(3) * silk * 0.38,
            )
        } else {
            // A small pan offset recentres the ring field, and width softens
            // the rings without changing their beat phase.
            let x = x - audio.balance * 0.06;
            let r = x.hypot(y);
            let a = y.atan2(x);
            let position = (a / TAU + 0.5).rem_euclid(1.0) * 31.0;
            let i = position.floor() as usize;
            let spectrum = bands[i] + (bands[(i + 1).min(31)] - bands[i]) * position.fract();
            let wobble = (a * 5.0 + t * 0.5).sin() * 0.07 + (a * 3.0 - t * 0.3).cos() * 0.055;
            let radius = r / (1.0 + audio.beat * 0.16 + bass * 0.10 + low_attack * 0.10)
                + wobble * (0.4 + r + mid * 0.4)
                - spectrum * 0.1
                - onset * 0.06
                - high_attack * 0.03 * (a * 11.0).sin();
            let wave = (radius * (14.0 + e * 2.0) - audio.locked_beat_phase() * TAU).sin();
            let rings = glow(wave, 0.085 + e * 0.06 + treble * 0.025 + audio.width * 0.02);
            let petals = (a * 8.0 + r * 3.0 - t * 0.3).cos() * 0.5 + 0.5;
            let core = glow(r, 0.13 + e * 0.05);
            (
                radius * 0.33 - t * 0.055 + petals * 0.13,
                0.07 + rings * (0.42 + petals * 0.5) + core * 0.5,
                rings * petals * 0.4 + core * 0.5,
            )
        };
        let base = theme.rgb(palette, phase, (light * (0.8 + e * 0.35)).clamp(0.0, 1.0));
        let rgb: [u8; 3] = std::array::from_fn(|i| {
            (base[i] as f32
                + (theme.foreground[i] as f32 - base[i] as f32) * highlight.clamp(0.0, 0.8))
            .round() as u8
        });
        Pixel {
            color: Color::Rgb(rgb[0], rgb[1], rgb[2]),
            mark: None,
        }
    }

    fn flower(
        &self,
        p: [f32; 2],
        vertices: &[[f32; 2]; 42],
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
    ) -> Pixel {
        let mut edge = f32::MAX;
        let mut tangent = [1.0, 0.0];
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let d = distance(p, a, b);
            if d < edge {
                edge = d;
                tangent = [b[0] - a[0], b[1] - a[1]];
            }
        }
        let ink = edge < (scale * 0.32).max(0.009);
        Pixel {
            color: theme.sample(
                palette,
                0.82 + audio.treble * 0.15,
                if ink { 0.96 } else { 0.035 },
            ),
            mark: ink.then(|| line_glyph(tangent)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scope(
        &self,
        p: [f32; 2],
        palette: usize,
        audio: &Reactivity,
        theme: &crate::palette::Theme,
        scale: f32,
        half_width: f32,
    ) -> Pixel {
        // Pixel centers of the first and last canvas columns are the endpoints.
        let span = (half_width - scale * 0.25).max(scale * 0.25);
        let position = ((p[0] / span + 1.0) * 0.5 * 31.0).clamp(0.0, 31.0);
        let index = (position.floor() as usize).min(30);
        let mut edge = f32::MAX;
        let mut tangent = [1.0, 0.0];
        for i in index.saturating_sub(1)..=(index + 1).min(30) {
            // Only real PCM determines the path; no synthetic oscillation.
            let a = [
                ((i as f32 / 31.0) * 2.0 - 1.0) * span,
                audio.waveform[i].clamp(-1.0, 1.0) * 0.88,
            ];
            let b = [
                (((i + 1) as f32 / 31.0) * 2.0 - 1.0) * span,
                audio.waveform[i + 1].clamp(-1.0, 1.0) * 0.88,
            ];
            let d = distance(p, a, b);
            if d < edge {
                edge = d;
                tangent = [b[0] - a[0], b[1] - a[1]];
            }
        }
        let ink = edge < (scale * 0.32).max(0.008);
        Pixel {
            color: theme.sample(
                palette,
                0.32 + p[0] / span * 0.12,
                if ink { 1.0 } else { 0.035 },
            ),
            mark: ink.then(|| line_glyph(tangent)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_scenes_use_continuous_half_block_shading() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        for scene in [2, 3, 4, 5] {
            let mut buffer = Buffer::empty(area);
            Renderer::default().draw(
                &mut buffer,
                area,
                0.0,
                scene,
                0,
                &[0.0; 32],
                &Reactivity::default(),
                &theme,
            );
            assert!(buffer.content.iter().all(|cell| cell.symbol() == "▀"));
            let shades: std::collections::HashSet<_> = buffer
                .content
                .iter()
                .flat_map(|cell| [cell.fg, cell.bg])
                .collect();
            assert!(
                shades.len() > 80,
                "soft scenes need rich shading, not color bands"
            );
        }
    }

    #[test]
    fn soft_scenes_respond_to_each_audio_region_and_the_active_theme() {
        let area = Rect::new(0, 0, 80, 28);
        let theme = crate::palette::Theme::default();
        let custom = crate::palette::Theme {
            ansi: [[100, 120, 140]; 16],
            background: [20, 30, 40],
            foreground: [150, 160, 170],
        };
        for scene in [2, 3, 4, 5] {
            let render = |audio: &Reactivity, theme: &crate::palette::Theme| {
                let mut renderer = Renderer::default();
                let mut buffer = Buffer::empty(area);
                for frame in 0..=12 {
                    renderer.draw(
                        &mut buffer,
                        area,
                        frame as f32 / 60.0,
                        scene,
                        0,
                        &[0.0; 32],
                        audio,
                        theme,
                    );
                }
                buffer
            };
            let quiet = render(&Reactivity::default(), &theme);
            for audio in [
                Reactivity {
                    bass: 0.8,
                    ..Reactivity::default()
                },
                Reactivity {
                    mid: 0.8,
                    ..Reactivity::default()
                },
                Reactivity {
                    treble: 0.8,
                    ..Reactivity::default()
                },
            ] {
                let active = render(&audio, &theme);
                let changed = quiet
                    .content
                    .iter()
                    .zip(&active.content)
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    changed > area.area() as usize / 10,
                    "each audio region must visibly affect soft scene {scene}"
                );
            }
            let themed = render(&Reactivity::default(), &custom);
            assert_ne!(quiet, themed);
            for color in themed.content.iter().flat_map(|cell| [cell.fg, cell.bg]) {
                let Color::Rgb(r, g, b) = color else {
                    panic!("RGB shading expected")
                };
                assert!((20..=150).contains(&r));
                assert!((30..=160).contains(&g));
                assert!((40..=170).contains(&b));
            }
        }
    }

    #[test]
    fn restored_designs_react_to_attacks_on_the_first_frame() {
        let area = Rect::new(0, 0, 100, 36);
        let theme = crate::palette::Theme::default();
        for scene in [2, 3, 4, 5] {
            let render = |audio: &Reactivity| {
                let mut buffer = Buffer::empty(area);
                Renderer::default().draw(
                    &mut buffer,
                    area,
                    2.0,
                    scene,
                    0,
                    &[0.0; 32],
                    audio,
                    &theme,
                );
                buffer
            };
            let quiet = render(&Reactivity::default());
            for transients in [[0.85, 0.0, 0.0, 0.0], [0.0, 0.0, 0.85, 0.85]] {
                let hit = render(&Reactivity {
                    transients,
                    ..Reactivity::default()
                });
                let changed = quiet
                    .content
                    .iter()
                    .zip(&hit.content)
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    changed > area.area() as usize / 5,
                    "attack must deform the design immediately"
                );
            }
        }
    }

    #[test]
    fn stereo_width_and_balance_reach_the_restored_scenes() {
        let area = Rect::new(0, 0, 100, 36);
        let theme = crate::palette::Theme::default();
        let render = |audio: &Reactivity| {
            let mut buffer = Buffer::empty(area);
            Renderer::default().draw(&mut buffer, area, 2.0, 3, 0, &[0.0; 32], audio, &theme);
            buffer
        };
        let centred = render(&Reactivity::default());
        let panned = render(&Reactivity {
            balance: 0.8,
            ..Reactivity::default()
        });
        let wide = render(&Reactivity {
            width: 0.8,
            ..Reactivity::default()
        });
        assert_ne!(centred, panned, "pan must move the curtain");
        assert_ne!(centred, wide, "width must change the ribbon");
    }

    #[test]
    fn swarm_has_discrete_heads_and_tails_and_reacts_to_attacks() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let render = |time, audio: &Reactivity| {
            let mut buffer = Buffer::empty(area);
            Renderer::default().draw(&mut buffer, area, time, 6, 0, &[0.6; 32], audio, &theme);
            buffer
        };
        let quiet = render(2.0, &Reactivity::default());
        assert!(quiet.content.iter().any(|cell| cell.symbol() == "●"));
        assert!(quiet
            .content
            .iter()
            .any(|cell| matches!(cell.symbol(), "─" | "╲" | "╱")));
        assert!(
            quiet
                .content
                .iter()
                .filter(|cell| cell.symbol() == " ")
                .count()
                > area.area() as usize * 3 / 4
        );
        for transients in [[0.9, 0.0, 0.0, 0.0], [0.0, 0.0, 0.9, 0.9]] {
            let hit = render(
                2.0,
                &Reactivity {
                    transients,
                    ..Reactivity::default()
                },
            );
            assert!(
                quiet
                    .content
                    .iter()
                    .zip(&hit.content)
                    .filter(|(a, b)| a.symbol() != b.symbol())
                    .count()
                    > 40
            );
        }
        assert_ne!(quiet, render(3.0, &Reactivity::default()));
    }

    #[test]
    fn pcm_moves_the_visible_trace_at_equal_energy() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let mut flat = Buffer::empty(area);
        let mut wriggly = Buffer::empty(area);
        let audio = Reactivity::default();
        let wave = Reactivity {
            waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin()),
            ..audio
        };
        Renderer::default().draw(&mut flat, area, 0.0, 1, 0, &[0.0; 32], &audio, &theme);
        Renderer::default().draw(&mut wriggly, area, 0.0, 1, 0, &[0.0; 32], &wave, &theme);
        let changed = flat
            .content
            .iter()
            .zip(&wriggly.content)
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 100,
            "PCM must change visible geometry, not a hidden input"
        );
    }

    #[test]
    fn original_scenes_are_distinct_and_pcm_belongs_only_to_scope() {
        let area = Rect::new(0, 0, 100, 36);
        let theme = crate::palette::Theme::default();
        let quiet = Reactivity {
            energy: 0.6,
            bass: 0.5,
            mid: 0.4,
            treble: 0.3,
            ..Reactivity::default()
        };
        let wave = Reactivity {
            waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin()),
            ..quiet
        };
        let mut scenes = Vec::new();
        for (scene, name) in ORIGINAL_SCENES.iter().enumerate() {
            let mut plain = Buffer::empty(area);
            let mut pcm = Buffer::empty(area);
            Renderer::default().draw(&mut plain, area, 0.0, scene, 0, &[0.5; 32], &quiet, &theme);
            Renderer::default().draw(&mut pcm, area, 0.0, scene, 0, &[0.5; 32], &wave, &theme);
            if scene == 1 {
                assert_ne!(plain, pcm);
            } else {
                assert_eq!(plain, pcm, "PCM must not leak into {name}");
            }
            scenes.push(plain);
        }
        for a in 0..scenes.len() {
            for b in a + 1..scenes.len() {
                assert_ne!(scenes[a], scenes[b], "scenes must not be duplicate layers");
            }
        }
    }

    #[test]
    fn flower_and_scope_leave_the_corners_empty() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        for scene in [0, 1] {
            let mut buf = Buffer::empty(area);
            Renderer::default().draw(
                &mut buf,
                area,
                0.0,
                scene,
                0,
                &[0.5; 32],
                &Reactivity::default(),
                &theme,
            );
            let corner = &buf[(0, 0)];
            assert_eq!(corner.symbol(), "▀");
            assert_eq!(corner.fg, corner.bg, "no field underneath the geometry");
        }
    }

    #[test]
    fn scope_pcm_endpoints_reach_both_canvas_edges() {
        let theme = crate::palette::Theme::default();
        for (width, height) in [(120, 40), (80, 24), (160, 50)] {
            let area = Rect::new(3, 2, width, height);
            let mut buf = Buffer::empty(area);
            let audio = Reactivity {
                waveform: std::array::from_fn(|i| if i % 2 == 0 { 0.75 } else { -0.75 }),
                ..Reactivity::default()
            };
            Renderer::default().draw(&mut buf, area, 0.0, 1, 0, &[0.0; 32], &audio, &theme);
            for x in [area.x, area.right() - 1] {
                assert!(
                    (area.y..area.bottom()).any(|y| buf[(x, y)].symbol() != "▀"),
                    "live PCM must reach column {x} at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn one_transient_fires_only_its_own_impact_channel() {
        let mut renderer = Renderer::default();
        renderer.advance(0.0, &Reactivity::default());
        renderer.advance(
            0.02,
            &Reactivity {
                transients: [0.9, 0.0, 0.0, 0.0],
                ..Reactivity::default()
            },
        );
        assert_eq!(renderer.impacts.len(), 1);
        assert_eq!(renderer.impacts[0].channel, 0);

        let mut broad = Renderer::default();
        broad.advance(0.0, &Reactivity::default());
        broad.advance(
            0.02,
            &Reactivity {
                onset: 1.0,
                kick: 1.0,
                novelty: 1.0,
                treble: 1.0,
                ..Reactivity::default()
            },
        );
        assert!(
            broad.impacts.is_empty(),
            "broad envelopes must not retrigger every transient channel"
        );
    }

    #[test]
    fn beat_phase_is_gated_by_tempo_confidence() {
        let audio = Reactivity {
            beat_phase: 0.75,
            tempo_confidence: 0.1,
            ..Reactivity::default()
        };
        assert_eq!(audio.locked_beat_phase(), 0.0);
        let locked = Reactivity {
            tempo_confidence: 0.9,
            ..audio
        };
        assert_eq!(locked.locked_beat_phase(), 0.75);
    }

    #[test]
    fn independent_attacks_persist_and_recoil() {
        let mut renderer = Renderer::default();
        let quiet = Reactivity::default();
        renderer.advance(0.0, &quiet);
        let hit = Reactivity {
            transients: [0.9, 0.8, 0.7, 0.6],
            ..quiet
        };
        renderer.advance(0.02, &hit);
        assert_eq!(renderer.impacts.len(), 4);
        let expansion = renderer.expansion;
        renderer.advance(0.04, &quiet);
        assert_eq!(renderer.impacts.len(), 4);
        assert!(renderer.expansion > expansion);
        for i in 3..100 {
            renderer.advance(i as f32 * 0.02, &quiet);
        }
        assert!(renderer.impacts.is_empty());
        assert!(renderer.expansion.abs() < 0.01);
    }

    #[test]
    fn motion_integrates_instead_of_multiplying_time_by_audio() {
        let mut early = Renderer::default();
        let mut late = Renderer::default();
        let quiet = Reactivity::default();
        let active = Reactivity {
            bass: 0.8,
            mid: 0.7,
            ..quiet
        };
        early.advance(0.0, &quiet);
        late.advance(1000.0, &quiet);
        early.advance(0.02, &active);
        late.advance(1000.02, &active);
        assert!((early.rotation - late.rotation).abs() < 0.0001);
    }

    #[test]
    fn short_attacks_change_geometry_and_clear_before_the_next_phrase() {
        let area = Rect::new(0, 0, 120, 40);
        let theme = crate::palette::Theme::default();
        let quiet = Reactivity::default();
        let hit = Reactivity {
            transients: [0.95; 4],
            ..quiet
        };
        let mut renderer = Renderer::default();
        let mut idle = Renderer::default();
        let mut struck = Buffer::empty(area);
        let mut plain = Buffer::empty(area);
        renderer.advance(0.0, &quiet);
        idle.advance(0.0, &quiet);
        renderer.draw(&mut struck, area, 0.016, 0, 0, &[0.0; 32], &hit, &theme);
        idle.draw(&mut plain, area, 0.016, 0, 0, &[0.0; 32], &quiet, &theme);
        assert!(renderer.expansion > 0.02);
        assert!(
            struck
                .content
                .iter()
                .zip(&plain.content)
                .filter(|(a, b)| a.symbol() != b.symbol())
                .count()
                > 15
        );
        for i in 2..=13 {
            renderer.advance(i as f32 * 0.016, &quiet);
        }
        assert!(renderer.impacts.is_empty());
        assert!(renderer.expansion.abs() < 0.005);
    }

    #[test]
    #[ignore = "release-only switching timing, excludes terminal I/O"]
    fn renderer_switch_latency() {
        let theme = crate::palette::Theme::default();
        let audio = Reactivity::default();
        let area = Rect::new(0, 0, 120, 40);
        let mut buffer = Buffer::empty(area);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(Renderer::default());
        }
        eprintln!(
            "constructor: {:.3} ms",
            start.elapsed().as_secs_f64() * 10.0
        );
        let mut renderer = Renderer::default();
        for scene in [2, 17, 18, 2, 17, 18] {
            let start = std::time::Instant::now();
            renderer.draw(&mut buffer, area, 1.0, scene, 0, &[0.4; 32], &audio, &theme);
            eprintln!(
                "switch to {}: {:.3} ms",
                SCENES[scene],
                start.elapsed().as_secs_f64() * 1000.0
            );
            std::hint::black_box(&buffer);
        }
    }

    #[test]
    fn catalog_has_two_hundred_unique_names() {
        let names: std::collections::HashSet<_> = SCENES.iter().copied().collect();
        assert_eq!(SCENES.len(), SCENE_COUNT);
        assert_eq!(names.len(), SCENES.len());
        assert!(!names.contains(""));
    }

    #[test]
    fn every_scene_has_unique_valid_tags() {
        let allowed: std::collections::HashSet<_> = TAGS.iter().copied().collect();
        assert_eq!(allowed.len(), TAGS.len(), "TAGS must not repeat");
        for (scene, name) in SCENES.iter().enumerate() {
            let tags = tags(scene);
            assert!(!tags.is_empty(), "{name} has no tags");
            let seen: std::collections::HashSet<_> = tags.iter().copied().collect();
            assert_eq!(seen.len(), tags.len(), "{name} repeats a tag");
            for tag in tags {
                assert!(allowed.contains(tag), "{name} uses unknown tag {tag}");
            }
        }
    }

    #[test]
    fn registry_maps_every_scene_once_to_a_family_and_name() {
        use std::collections::{HashMap, HashSet};

        let expected: HashSet<Family> = ORIGINAL_FAMILIES
            .iter()
            .map(|(family, _)| *family)
            .chain(NEW_FAMILIES.iter().map(|(family, _)| *family))
            .collect();
        let mut routes = HashSet::new();
        let mut locals: HashMap<Family, Vec<usize>> = HashMap::new();
        for (index, name) in SCENES.iter().enumerate() {
            let (family, local) = scene_route(index);
            assert_eq!(REGISTRY[index].name, *name);
            assert!(routes.insert((family, local)), "duplicate route at {index}");
            locals.entry(family).or_default().push(local);
        }
        assert_eq!(SCENES.len(), SCENE_COUNT);
        assert_eq!(routes.len(), SCENE_COUNT);
        assert_eq!(locals.len(), expected.len());
        assert_eq!(locals.keys().copied().collect::<HashSet<_>>(), expected);
        for (family, indexes) in locals.iter_mut() {
            indexes.sort_unstable();
            let expected_indexes: Vec<_> = (0..indexes.len()).collect();
            assert_eq!(indexes, &expected_indexes, "{family:?}");
        }
    }

    #[test]
    fn original_special_scenes_route_to_their_families() {
        for scene in 0..=6 {
            assert_eq!(scene_route(scene), (Family::Original, scene));
        }
        let area = Rect::new(2, 3, 24, 12);
        let theme = crate::palette::Theme::default();
        let audio = Reactivity {
            energy: 0.6,
            bass: 0.5,
            mid: 0.4,
            treble: 0.3,
            transients: [0.7; 4],
            ..Reactivity::default()
        };
        macro_rules! check_legacy {
            ($first:expr, $family:ident, $kind:ident) => {
                for local in 0..2 {
                    let scene = $first + local;
                    assert_eq!(scene_route(scene), (Family::$kind, local));
                    let mut routed = Buffer::empty(area);
                    let mut direct = Buffer::empty(area);
                    Renderer::default().draw(
                        &mut routed,
                        area,
                        1.0,
                        scene,
                        0,
                        &[0.5; 32],
                        &audio,
                        &theme,
                    );
                    crate::families::$family::Renderer::default().draw(
                        &mut direct,
                        area,
                        1.0,
                        local,
                        0,
                        &[0.5; 32],
                        &audio,
                        &theme,
                    );
                    assert_eq!(routed, direct, "legacy dispatch for scene {scene}");
                }
            };
        }
        check_legacy!(7, glyphs, Glyphs);
        check_legacy!(9, sculptures, Sculptures);
        check_legacy!(11, flocks, Flocks);
        check_legacy!(13, spectrum, Spectrum);
        check_legacy!(15, fractals, Fractals);
        check_legacy!(17, chaos, Chaos);
    }

    #[test]
    fn catalog_routes_every_new_style_to_its_family() {
        let area = Rect::new(2, 3, 24, 12);
        let theme = crate::palette::Theme::default();
        let audio = Reactivity {
            energy: 0.6,
            bass: 0.5,
            mid: 0.4,
            treble: 0.3,
            transients: [0.7; 4],
            ..Reactivity::default()
        };
        let mut offset = ORIGINAL_SCENES.len();
        macro_rules! check_family {
            ($family:ident, $kind:ident) => {
                for (local, name) in crate::families::$family::NAMES.iter().enumerate() {
                    let scene = offset + local;
                    assert_eq!(SCENES[scene], *name);
                    assert_eq!(scene_route(scene), (Family::$kind, local));
                    let mut routed = Buffer::empty(area);
                    let mut direct = Buffer::empty(area);
                    Renderer::default().draw(
                        &mut routed,
                        area,
                        1.0,
                        scene,
                        0,
                        &[0.5; 32],
                        &audio,
                        &theme,
                    );
                    crate::families::$family::Renderer::default().draw(
                        &mut direct,
                        area,
                        1.0,
                        local,
                        0,
                        &[0.5; 32],
                        &audio,
                        &theme,
                    );
                    assert_eq!(routed, direct, "catalog dispatch for {name}");
                }
                offset += crate::families::$family::NAMES.len();
            };
        }
        check_family!(curves, Curves);
        check_family!(waves, Waves);
        check_family!(cells, Cells);
        check_family!(geometry, Geometry);
        check_family!(particles, Particles);
        check_family!(typography, Typography);
        check_family!(organic, Organic);
        check_family!(fractalmath, FractalMath);
        check_family!(circuit, Circuit);
        check_family!(metaballs, Metaballs);
        check_family!(fluid, Fluid);
        check_family!(tunnel, Tunnel);
        check_family!(kaleidoscope, Kaleidoscope);
        check_family!(weather, Weather);
        check_family!(terrain, Terrain);
        check_family!(textile, Textile);
        check_family!(orbital, Orbital);
        check_family!(city, City);
        assert_eq!(offset, SCENES.len());
    }

    #[test]
    fn only_the_visible_family_is_initialized_and_retained() {
        let mut renderer = Renderer::default();
        let area = Rect::new(0, 0, 8, 4);
        let mut buffer = Buffer::empty(area);
        let theme = crate::palette::Theme::default();
        let audio = Reactivity::default();
        for scene in [2, 7, 2] {
            renderer.draw(&mut buffer, area, 0.0, scene, 0, &[0.0; 32], &audio, &theme);
            assert!(renderer.curves.is_none());
            assert!(renderer.waves.is_none());
            assert!(renderer.cells.is_none());
            assert!(renderer.geometry.is_none());
            assert!(renderer.particles.is_none());
            assert!(renderer.typography.is_none());
            assert!(renderer.organic.is_none());
            assert!(renderer.fractalmath.is_none());
            assert!(renderer.circuit.is_none());
            assert!(renderer.metaballs.is_none());
            assert!(renderer.fluid.is_none());
            assert!(renderer.tunnel.is_none());
            assert!(renderer.kaleidoscope.is_none());
            assert!(renderer.weather.is_none());
            assert!(renderer.terrain.is_none());
            assert!(renderer.textile.is_none());
            assert!(renderer.orbital.is_none());
            assert!(renderer.city.is_none());
            assert!(renderer.chaos.is_none());
            assert!(renderer.fractals.is_none());
            assert!(renderer.spectrum.is_none());
            assert!(renderer.flocks.is_none());
            assert!(renderer.sculptures.is_none());
            if scene == 7 {
                assert!(renderer.glyphs.is_some());
            }
        }
        assert!(
            renderer.glyphs.is_some(),
            "switching away retains initialized state"
        );
    }

    #[test]
    #[ignore = "release-only renderer timing, excludes terminal I/O"]
    fn renderer_frame_budget() {
        for (scene, width, height) in
            (0..SCENES.len()).flat_map(|scene| [(scene, 120, 40), (scene, 160, 50)])
        {
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            let mut renderer = Renderer::default();
            let theme = crate::palette::Theme::default();
            let mut audio = Reactivity {
                energy: 0.8,
                bass: 0.7,
                mid: 0.6,
                treble: 0.8,
                waveform: std::array::from_fn(|i| (i as f32 * 1.3).sin() * 0.7),
                ..Reactivity::default()
            };
            let start = std::time::Instant::now();
            for i in 0..180 {
                audio.transients = [if i % 3 == 0 { 0.95 } else { 0.0 }; 4];
                renderer.draw(
                    &mut buffer,
                    area,
                    i as f32 / 60.0,
                    scene,
                    0,
                    &[0.6; 32],
                    &audio,
                    &theme,
                );
                std::hint::black_box(&buffer);
            }
            eprintln!(
                "{} {width}x{height}: {:.2} ms/frame",
                SCENES[scene],
                start.elapsed().as_secs_f64() * 1000.0 / 180.0
            );
        }
    }

    #[test]
    fn rapid_attacks_are_bounded_and_survive_silence() {
        let mut renderer = Renderer::default();
        for i in 0..100 {
            let value = if i % 2 == 0 { 1.0 } else { 0.0 };
            renderer.advance(
                i as f32 * 0.025,
                &Reactivity {
                    transients: [value; 4],
                    ..Reactivity::default()
                },
            );
        }
        assert!(renderer.impacts.len() <= 32);
        assert!(renderer.impacts.len() > 8);
    }
}
