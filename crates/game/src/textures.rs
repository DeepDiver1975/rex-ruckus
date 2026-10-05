//! Procedural 32×32 pixel-art textures, so M1 needs no downloaded art.
#![allow(clippy::manual_is_multiple_of)] // `% n == 0` reads clearer for pixel-grid maths

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const SIZE: u32 = 32;

/// Material name of crack walls (see `mechanics::mark_crack_walls`).
pub const CRACKED: &str = "cracked";

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
        // Placeholder pane: pale blue with diagonal glints (translucency comes later).
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
