//! Procedural 32×32 pixel-art textures, so M1 needs no downloaded art.
#![allow(clippy::manual_is_multiple_of)] // `% n == 0` reads clearer for pixel-grid maths

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const SIZE: u32 = 32;

/// Material name of crack walls (see `mechanics::mark_crack_walls`).
pub const CRACKED: &str = "cracked";

/// Material names of hazard floors (see `mechanics::mark_hazard_floors`).
pub const SLIME: &str = "slime";
pub const ELECTRIC: &str = "electric";
pub const MAGMA: &str = "magma";

/// Cheap deterministic integer hash → 0..=255 noise.
fn noise(x: u32, y: u32, seed: u32) -> u8 {
    let mut h = x.wrapping_mul(374_761_393)
        ^ y.wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as u8
}

fn shade(rgb: [u8; 3], amount: i32) -> [u8; 4] {
    let c = |v: u8| (v as i32 + amount).clamp(0, 255) as u8;
    [c(rgb[0]), c(rgb[1]), c(rgb[2]), 255]
}

fn texel(name: &str, x: u32, y: u32) -> [u8; 4] {
    let n = noise(x, y, name.len() as u32) as i32 / 16 - 8; // −8..7
    match name {
        "brick" => {
            let row = y / 8;
            let offset = if row % 2 == 0 { 0 } else { 8 };
            if y % 8 == 0 || (x + offset) % 16 == 0 {
                [150, 145, 135, 255] // flat mortar
            } else {
                shade([150, 60, 45], n * 2 + noise(x / 16, row, 7) as i32 / 24)
            }
        }
        "concrete" => shade([120, 120, 115], n * 2),
        // Concrete scarred by dark zig-zag fissures: the walls a blast can open.
        "cracked" => {
            let fissure = |cx: u32, phase: u32| {
                let wobble = (noise(y / 3, phase, 11) % 3) as i32 - 1;
                x as i32
                    == cx as i32 + (y as i32 % 8) / 2 * if y / 8 % 2 == 0 { 1 } else { -1 } + wobble
            };
            if fissure(8, 1) || fissure(23, 2) || (y == 15 && (4..28).contains(&x)) {
                [25, 25, 25, 255]
            } else {
                shade([105, 105, 100], n * 2)
            }
        }
        // Toxic green with lighter bubble rings.
        SLIME => shade(
            [60, 170, 40],
            n * 3 + if (x + 3 * y) % 11 == 0 { 45 } else { 0 },
        ),
        // Dark blue with a bright crackling zigzag.
        ELECTRIC => {
            if x == 4 + (y % 8) * 3 % 24 || y == 16 {
                [150, 220, 255, 255]
            } else {
                shade([20, 30, 70], n)
            }
        }
        // Pane: pale blue with diagonal glints; the level material blends it translucent.
        "glass" => shade([170, 205, 220], if (x + y) % 11 == 0 { 35 } else { n / 2 }),
        "sky" => shade([40, 60, 120], (SIZE - y) as i32 * 2 + n / 4),
        "metal" => {
            let seam = x % 16 == 0 || y % 16 == 0;
            let rivet = x % 16 == 2 && y % 16 == 2;
            shade(
                [95, 100, 110],
                if seam {
                    -35
                } else if rivet {
                    40
                } else {
                    n
                },
            )
        }
        "tile" => {
            if x % 8 == 0 || y % 8 == 0 {
                shade([90, 90, 90], n / 2)
            } else if (x / 8 + y / 8) % 2 == 0 {
                shade([220, 220, 215], n / 2)
            } else {
                shade([120, 170, 200], n / 2)
            }
        }
        "wood" => shade(
            [120, 80, 45],
            if x % 8 == 0 {
                -30
            } else {
                n * 2 + (y as i32 % 5) * 2
            },
        ),
        "door" => {
            // Metal panel with a hazard-striped band across the middle.
            if (12..20).contains(&y) {
                if ((x + y) / 4) % 2 == 1 {
                    [230, 180, 20, 255]
                } else {
                    [25, 25, 25, 255]
                }
            } else {
                shade([110, 115, 125], if x % 16 == 0 { -30 } else { n })
            }
        }
        // Street: dark noisy tarmac with a faint pale fleck.
        "asphalt" => shade(
            [48, 48, 52],
            n * 2 + if noise(x, y, 31) > 250 { 60 } else { 0 },
        ),
        // Roof gravel: warm grey speckle.
        "gravel" => shade([110, 100, 90], noise(x, y, 5) as i32 / 6 - 20),
        // Theater carpet: red with a gold diamond lattice.
        "carpet" => {
            let d = ((x as i32 - 16).abs() + (y as i32 % 16 - 8).abs()) % 16;
            if d == 0 {
                [200, 160, 60, 255]
            } else {
                shade([140, 20, 30], n)
            }
        }
        // Stage velvet: deep red vertical folds.
        "velvet" => shade([120, 10, 25], ((x % 8) as i32 - 4).abs() * 6 - 12 + n / 2),
        // Office ceiling: off-white tiles on a grey grid.
        "ceiling_tile" => {
            if x % 16 == 0 || y % 16 == 0 {
                [140, 140, 140, 255]
            } else {
                shade([215, 212, 200], n)
            }
        }
        // Ship hull: blue-grey plates, seams every 16 px, rivets at plate corners.
        "hull" => {
            if x % 16 == 0 || y % 16 == 0 {
                [40, 45, 55, 255]
            } else if x % 16 == 2 && y % 16 == 2 {
                [170, 175, 185, 255]
            } else {
                shade([85, 95, 110], n * 2)
            }
        }
        // Floor grating: metal bars around dark holes.
        "grate" => {
            if x % 4 == 0 || y % 4 == 0 {
                shade([120, 120, 125], n)
            } else {
                [15, 15, 18, 255]
            }
        }
        // Alien organic wall: purple with dark wandering veins.
        "flesh" => {
            let vein = (x as i32 + (noise(y / 2, 0, 9) % 5) as i32) % 11 == 0;
            if vein {
                [50, 10, 40, 255]
            } else {
                shade([130, 50, 110], n * 2)
            }
        }
        // Neon tubes: pink and cyan bands on black.
        "neon" => match y % 16 {
            3..=4 => [255, 60, 200, 255],
            11..=12 => [40, 230, 255, 255],
            _ => shade([15, 10, 20], n / 2),
        },
        // Marquee: warm bulbs on a dark panel.
        "marquee" => {
            if x % 8 == 4 && y % 8 == 4 {
                [255, 230, 140, 255]
            } else {
                shade([40, 25, 10], n)
            }
        }
        // Dance floor: lit 8 px tiles in four colours.
        "dancefloor" => {
            if x % 8 == 0 || y % 8 == 0 {
                [10, 10, 10, 255]
            } else {
                [
                    [220, 40, 160, 255],
                    [40, 200, 255, 255],
                    [250, 220, 40, 255],
                    [90, 255, 90, 255],
                ][((x / 8 + 2 * (y / 8)) % 4) as usize]
            }
        }
        // Glow strip: a bright cyan line along a dark band.
        "glowstrip" => {
            if (14..18).contains(&y) {
                [120, 255, 240, 255]
            } else {
                shade([30, 35, 45], n)
            }
        }
        // Shipping container: corrugated steel ribs in faded rust-red paint, a dark frame rail
        // top and bottom, rust streaks running down from the top and pale scuffs.
        "container" => {
            if !(2..30).contains(&y) {
                return shade([55, 40, 35], n);
            }
            if noise(x, y, 22) > 252 {
                return shade([170, 160, 150], n);
            }
            let rib = match x % 8 {
                0 | 1 => 28,
                2 | 7 => 12,
                4 | 5 => -22,
                _ => 0,
            };
            let streak = noise(x, 0, 21) > 200 && y < 4 + noise(x, 1, 21) as u32 / 10;
            if streak {
                shade([105, 50, 25], rib / 2 + n)
            } else {
                shade([150, 62, 38], rib + n)
            }
        }
        // Water: dark blue-green with wavy ripple crests and the odd bright glint.
        "water" => {
            let wave = (y + [0, 1, 2, 2, 1, 0, 0, 0][(x / 4 % 8) as usize]) % 8;
            let glint = noise(x, y, 61) > 253;
            let crest = match wave {
                _ if glint => 110,
                0 => 45,
                1 => 20,
                _ => 0,
            };
            shade([20, 62, 72], n + crest)
        }
        // Magma: black crust plates split by glowing cracks, white-yellow at the core and
        // orange at the edges.
        MAGMA => {
            // The cracks are the edges of a Voronoi tiling: pixels about as near to their
            // second-nearest crust seed as to their nearest. Distances wrap, so it tiles.
            let (mut d1, mut d2) = (f32::MAX, f32::MAX);
            for i in 0..7 {
                let (sx, sy) = (noise(i, 0, 53) as u32 % SIZE, noise(i, 1, 53) as u32 % SIZE);
                let wrap = |a: u32, b: u32| {
                    let d = a.abs_diff(b);
                    d.min(SIZE - d) as f32
                };
                let d = wrap(x, sx).hypot(wrap(y, sy));
                if d < d1 {
                    (d1, d2) = (d, d1);
                } else if d < d2 {
                    d2 = d;
                }
            }
            match d2 - d1 {
                e if e < 1.0 => [255, 220, 90, 255],
                e if e < 2.2 => [235, 110, 15, 255],
                _ => shade([32, 20, 16], n),
            }
        }
        // Hive: waxy plum honeycomb domes on dark veins, an amber pore in each cell.
        "hive" => {
            let row = y / 8;
            let lx = (x + if row % 2 == 0 { 0 } else { 4 }) % 8;
            let (dx, dy) = (lx as i32 - 4, (y % 8) as i32 - 4);
            let d = dx.abs() + dy.abs();
            if dx == -4 || d >= 6 {
                shade([45, 14, 42], n / 2)
            } else if d == 0 {
                [220, 150, 50, 255]
            } else {
                shade([100, 42, 92], 24 - d * 6 + n)
            }
        }
        // Monitor: an original test screen (four colour bars over a grey step ramp, a white
        // ring in the middle) on dark scanlines, framed by a thin black bezel.
        "screen" => {
            if x == 0 || y == 0 {
                return [10, 10, 12, 255];
            }
            let (dx, dy) = (x as i32 - 16, y as i32 - 16);
            let r2 = dx * dx + dy * dy;
            let c = if (49..=72).contains(&r2) {
                [245, 245, 245]
            } else if y < 22 {
                [
                    [40, 220, 230],
                    [230, 60, 200],
                    [250, 190, 40],
                    [120, 240, 70],
                ][(x / 8) as usize]
            } else {
                let g = 40 + (x / 4) as u8 * 26;
                [g, g, g]
            };
            shade(c, if y % 2 == 1 { -60 } else { 0 })
        }
        // Monorail beam: a polished top flange, a dark bolted web, a lower flange and shadow
        // lines between them.
        "rail" => match y {
            0..=1 | 30..=31 => [20, 20, 24, 255],
            2..=5 => shade([195, 195, 200], if y == 2 { 30 } else { n }),
            26..=29 => shade([145, 148, 155], n),
            _ if x % 8 == 4 && (y == 10 || y == 21) => [210, 205, 170, 255],
            _ if x % 8 == 5 && (y == 11 || y == 22) => [30, 30, 32, 255],
            _ if y == 6 || y == 25 => [55, 58, 64, 255],
            _ => shade([88, 93, 102], n),
        },
        // Shop front: dark panes in bright aluminium frames over a brushed kick panel, goods
        // on two shelves behind the glass.
        "storefront" => {
            if x % 16 < 2 || y < 2 || (20..22).contains(&y) {
                return shade(
                    [205, 210, 215],
                    if x % 16 == 1 || y == 1 || y == 21 {
                        -40
                    } else {
                        0
                    },
                );
            }
            if y >= 22 {
                return shade([150, 152, 158], (x % 3) as i32 * 6 - 6);
            }
            if y == 11 || y == 19 {
                return [70, 75, 80, 255];
            }
            let slot = x / 4;
            let shelf = if y < 11 { 0 } else { 1 };
            let item = noise(slot, shelf, 71);
            let top = if shelf == 0 {
                5 + item as u32 % 4
            } else {
                13 + item as u32 % 4
            };
            if x % 4 != 0 && item > 70 && y >= top {
                // Dimmed by the glass.
                let c = [[200, 60, 50], [60, 120, 200], [230, 200, 60], [90, 180, 90]]
                    [(item % 4) as usize];
                return [c[0] / 2 + 20, c[1] / 2 + 20, c[2] / 2 + 20, 255];
            }
            shade([38, 58, 72], if (x + y) % 13 == 0 { 45 } else { n / 2 })
        }
        // Pipe run: a yellow and a grey pipe, rounded by shading, with bolted flanges every
        // 16 px and the odd paint chip.
        "pipe" => {
            let ly = (y % 16) as i32;
            if ly == 0 || ly == 15 {
                return [18, 18, 20, 255];
            }
            let base = if y < 16 {
                [205, 165, 35]
            } else {
                [125, 130, 138]
            };
            let round = 26 - (ly - 5).abs() * 7;
            if x % 16 < 3 {
                if x % 16 == 1 && (ly == 3 || ly == 12) {
                    return [225, 225, 215, 255];
                }
                return shade(base, round - 45);
            }
            if noise(x, y, 81) > 247 {
                return shade([90, 90, 95], n);
            }
            shade(base, round + n)
        }
        _ => {
            if (x / 8 + y / 8) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        }
    }
}

/// Raw RGBA8 bytes for a named texture (unknown names get a magenta "missing" checker).
pub fn pixels(name: &str) -> Vec<u8> {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).flat_map(move |x| texel(name, x, y)))
        .collect()
}

pub fn generate(name: &str) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels(name),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler = ImageSamplerDescriptor::nearest();
    sampler.address_mode_u = ImageAddressMode::Repeat;
    sampler.address_mode_v = ImageAddressMode::Repeat;
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_are_deterministic_32px_rgba() {
        for name in [
            "brick", "concrete", "sky", "metal", "tile", "wood", "door", "cracked", "glass",
        ] {
            let a = pixels(name);
            assert_eq!(a.len(), (SIZE * SIZE * 4) as usize, "{name}");
            assert_eq!(a, pixels(name), "{name} must be deterministic");
        }
    }

    const THEMED: [&str; 12] = [
        "asphalt",
        "gravel",
        "carpet",
        "velvet",
        "ceiling_tile",
        "hull",
        "grate",
        "flesh",
        "neon",
        "marquee",
        "dancefloor",
        "glowstrip",
    ];

    #[test]
    fn themed_textures_are_distinct_and_deterministic() {
        let missing = pixels("does-not-exist");
        for (i, a) in THEMED.iter().enumerate() {
            let p = pixels(a);
            assert_eq!(p.len(), (SIZE * SIZE * 4) as usize, "{a}");
            assert_eq!(p, pixels(a), "{a} deterministic");
            assert_ne!(p, missing, "{a} has its own arm");
            for b in &THEMED[i + 1..] {
                assert_ne!(p, pixels(b), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn every_known_material_has_its_own_texture() {
        let missing = pixels("does-not-exist");
        for name in rr_core::map::KNOWN_MATERIALS {
            assert_ne!(
                pixels(name),
                missing,
                "{name} falls back to the missing checker"
            );
        }
    }

    #[test]
    fn hazard_textures_differ_from_each_other_and_the_checker() {
        let (s, e, missing) = (pixels(SLIME), pixels(ELECTRIC), pixels("does-not-exist"));
        assert_eq!(s.len(), (SIZE * SIZE * 4) as usize);
        assert_ne!(s, e);
        assert_ne!(s, missing);
        assert_ne!(e, missing);
        assert_eq!(s, pixels(SLIME), "deterministic");
    }

    /// The Episode 2 ("Aftershock") materials.
    const AFTERSHOCK: [&str; 8] = [
        "container",
        "water",
        MAGMA,
        "hive",
        "screen",
        "rail",
        "storefront",
        "pipe",
    ];

    #[test]
    fn aftershock_textures_are_deterministic_non_uniform_and_distinct() {
        let missing = pixels("does-not-exist");
        for (i, a) in AFTERSHOCK.iter().enumerate() {
            let p = pixels(a);
            assert_eq!(p.len(), (SIZE * SIZE * 4) as usize, "{a}");
            assert_eq!(p, pixels(a), "{a} deterministic");
            assert_ne!(p, missing, "{a} has its own arm");
            assert!(p.chunks(4).all(|px| px[3] == 255), "{a} is opaque");
            let colours: std::collections::BTreeSet<_> = p.chunks(4).collect();
            assert!(colours.len() >= 8, "{a} has only {} colours", colours.len());
            for b in AFTERSHOCK[i + 1..].iter().chain(&THEMED) {
                assert_ne!(p, pixels(b), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn magma_is_bright_cracks_over_dark_crust() {
        let p = pixels(MAGMA);
        let luma = |px: &[u8]| px[0] as u32 + px[1] as u32 + px[2] as u32;
        let bright = p.chunks(4).filter(|px| luma(px) > 400).count();
        let dark = p.chunks(4).filter(|px| luma(px) < 120).count();
        assert!(bright > 60 && dark > 400, "bright {bright}, dark {dark}");
    }

    #[test]
    fn hive_differs_from_flesh_and_has_amber_pores() {
        let p = pixels("hive");
        assert_ne!(p, pixels("flesh"));
        let pores = p.chunks(4).filter(|px| px[..3] == [220, 150, 50]).count();
        assert_eq!(pores, 16, "one pore per honeycomb cell");
    }

    /// Writes a swatch sheet of the Aftershock materials (and one PNG each) to
    /// `$RR_SWATCH_DIR`, else `$TMPDIR/rr-swatches`: `cargo test -p rr-game --lib --
    /// --ignored write_aftershock_swatches`. The sheet is a 4×2 grid in `AFTERSHOCK` order, each
    /// tile the texture repeated 2×2 and scaled 4× (nearest), as the game samples it.
    #[test]
    #[ignore = "writes PNGs; run by hand for PR screenshots"]
    fn write_aftershock_swatches() {
        const REP: u32 = 2;
        const ZOOM: u32 = 4;
        const GAP: u32 = 8;
        let tile = SIZE * REP * ZOOM;
        let dir = std::env::var_os("RR_SWATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("rr-swatches"));
        std::fs::create_dir_all(&dir).unwrap();
        let png = |w: u32, h: u32, data: Vec<u8>, name: &str| {
            let img = Image::new(
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            let path = dir.join(format!("{name}.png"));
            img.try_into_dynamic().unwrap().save(&path).unwrap();
            println!("{}", path.display());
        };
        let (cols, rows) = (4, 2);
        let (w, h) = (cols * (tile + GAP) + GAP, rows * (tile + GAP) + GAP);
        let mut sheet = [24u8, 24, 28, 255].repeat((w * h) as usize);
        for (i, name) in AFTERSHOCK.iter().enumerate() {
            let p = pixels(name);
            let src = |x: u32, y: u32| {
                let o = (((y / ZOOM % SIZE) * SIZE + x / ZOOM % SIZE) * 4) as usize;
                &p[o..o + 4]
            };
            let one: Vec<u8> = (0..tile)
                .flat_map(|y| (0..tile).flat_map(move |x| src(x, y).to_vec()))
                .collect();
            png(tile, tile, one, name);
            let (ox, oy) = (
                GAP + (i as u32 % cols) * (tile + GAP),
                GAP + (i as u32 / cols) * (tile + GAP),
            );
            for y in 0..tile {
                for x in 0..tile {
                    let o = (((oy + y) * w + ox + x) * 4) as usize;
                    sheet[o..o + 4].copy_from_slice(src(x, y));
                }
            }
        }
        png(w, h, sheet, "aftershock_swatches");
    }

    #[test]
    fn unknown_name_gives_missing_texture_checker() {
        let p = pixels("does-not-exist");
        assert_eq!(&p[0..4], &[255, 0, 255, 255]);
    }

    #[test]
    fn cracked_concrete_has_dark_fissures_unlike_plain_concrete() {
        let (c, p) = (pixels(CRACKED), pixels("concrete"));
        assert_ne!(c, p);
        let dark = c.chunks(4).filter(|px| px[0] < 40).count();
        assert!(dark > 30, "fissure pixels: {dark}");
    }

    #[test]
    fn door_has_hazard_band() {
        let p = pixels("door");
        let px = |x: u32, y: u32| &p[((y * SIZE + x) * 4) as usize..][..3];
        assert_eq!(px(0, 12), &[230, 180, 20]);
        assert_eq!(px(4, 12), &[25, 25, 25]);
    }

    #[test]
    fn brick_has_mortar_rows() {
        let p = pixels("brick");
        let px = |x: u32, y: u32| &p[((y * SIZE + x) * 4) as usize..][..3];
        assert_eq!(px(5, 0), px(20, 0), "row 0 is mortar everywhere");
        assert_ne!(px(5, 3), px(5, 0), "brick body differs from mortar");
    }
}
