# Credits

All code and procedural content (including the synthesised sounds) are original.
Textures are generated procedurally at runtime; effects, switch panels, the boot, the
kick leg and the pipe-bomb fist are built from Bevy primitives at runtime. The third-party CC0 models, the CC0
recordings for sound effects and music, and the text-to-speech model used to generate the
quips are listed below with their source URL and licence.

All third-party files below are CC0 1.0 (public domain dedication,
https://creativecommons.org/publicdomain/zero/1.0/). Each sound was converted with
`scripts/import-sfx.sh` (mono OGG Vorbis, quality 4, leading silence trimmed, peak
normalised to -1 dBFS); extra processing is noted per row. Loops (`LOOP=`) are a cut
whose following 0.25 s is crossfaded into its head.

## Sound effects

Sources:

- **Kenney Impact Sounds** by Kenney (www.kenney.nl), https://kenney.nl/assets/impact-sounds, CC0
- **Kenney Sci-Fi Sounds** by Kenney (www.kenney.nl), https://kenney.nl/assets/sci-fi-sounds, CC0
- **75 CC0 breaking / falling / hit sfx** by rubberduck, https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx, CC0
- **80 CC0 creature SFX** by rubberduck, https://opengameart.org/content/80-cc0-creature-sfx, CC0
- **Death sounds** by Exewin, https://opengameart.org/content/death-sounds-0, CC0

| Repo path | Pack: original file | Author | Source | Licence | Processing |
|---|---|---|---|---|---|
| `assets/sounds/cc0/door_start.ogg` | Sci-Fi Sounds: `spaceEngineSmall_000.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | `pitch 500`, `LOOP="0.5 2.0"` |
| `assets/sounds/cc0/door_stop.ogg` | Sci-Fi Sounds: `doorClose_000.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | none |
| `assets/sounds/cc0/lift_start.ogg` | Sci-Fi Sounds: `spaceEngineLow_000.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | `LOOP="1.0 3.0"` |
| `assets/sounds/cc0/lift_stop.ogg` | Impact Sounds: `impactPlate_heavy_000.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/footstep_1.ogg` | Impact Sounds: `footstep_concrete_000.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/footstep_2.ogg` | Impact Sounds: `footstep_concrete_001.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/footstep_3.ogg` | Impact Sounds: `footstep_concrete_002.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/footstep_4.ogg` | Impact Sounds: `footstep_concrete_003.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/land.ogg` | Impact Sounds: `impactSoft_heavy_000.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/impact_1.ogg` | Impact Sounds: `impactMetal_light_000.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/impact_2.ogg` | Impact Sounds: `impactMetal_medium_001.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/impact_3.ogg` | Impact Sounds: `impactGeneric_light_000.ogg` | Kenney | https://kenney.nl/assets/impact-sounds | CC0 | none |
| `assets/sounds/cc0/explosion_1.ogg` | Sci-Fi Sounds: `lowFrequency_explosion_000.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | none |
| `assets/sounds/cc0/explosion_2.ogg` | Sci-Fi Sounds: `explosionCrunch_004.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | none |
| `assets/sounds/cc0/jetpack_loop.ogg` | Sci-Fi Sounds: `thrusterFire_000.ogg` | Kenney | https://kenney.nl/assets/sci-fi-sounds | CC0 | `LOOP="1.0 2.0"` |
| `assets/sounds/cc0/glass_break_1.ogg` | 75 CC0 breaking / falling / hit sfx: `bfh1_glass_breaking_01.ogg` | rubberduck | https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx | CC0 | none |
| `assets/sounds/cc0/glass_break_2.ogg` | 75 CC0 breaking / falling / hit sfx: `bfh1_glass_breaking_03.ogg` | rubberduck | https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx | CC0 | none |
| `assets/sounds/cc0/actor_wake_grunt.ogg` | 80 CC0 creature SFX: `grunt_01.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | none |
| `assets/sounds/cc0/actor_pain_grunt.ogg` | 80 CC0 creature SFX: `hurt_01.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | none |
| `assets/sounds/cc0/actor_death_grunt.ogg` | 80 CC0 creature SFX: `monster_01.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | none |
| `assets/sounds/cc0/actor_wake_enforcer.ogg` | 80 CC0 creature SFX: `roar_02.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch -300 overdrive 4` |
| `assets/sounds/cc0/actor_pain_enforcer.ogg` | 80 CC0 creature SFX: `hurt_02.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch -400 overdrive 4` |
| `assets/sounds/cc0/actor_death_enforcer.ogg` | 80 CC0 creature SFX: `monster_04.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch -300 overdrive 4` |
| `assets/sounds/cc0/actor_wake_slasher.ogg` | 80 CC0 creature SFX: `troll_02.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch 200 tempo 1.15` |
| `assets/sounds/cc0/actor_pain_slasher.ogg` | 80 CC0 creature SFX: `hurt_03.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch 300` |
| `assets/sounds/cc0/actor_death_slasher.ogg` | 80 CC0 creature SFX: `scream_02.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch 200` |
| `assets/sounds/cc0/actor_wake_drone.ogg` | 80 CC0 creature SFX: `alien_01.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `overdrive 12 tremolo 45 70` |
| `assets/sounds/cc0/actor_pain_drone.ogg` | 80 CC0 creature SFX: `alien_05.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `overdrive 12 tremolo 45 70` |
| `assets/sounds/cc0/actor_death_drone.ogg` | 80 CC0 creature SFX: `alien_02.ogg` | rubberduck | https://opengameart.org/content/80-cc0-creature-sfx | CC0 | `pitch -500 overdrive 12 tremolo 30 70` |
| `assets/sounds/cc0/player_hurt_1.ogg` | Death sounds: `exewinDeathSoundsPack/3.ogg` | Exewin | https://opengameart.org/content/death-sounds-0 | CC0 | none |
| `assets/sounds/cc0/player_hurt_2.ogg` | Death sounds: `exewinDeathSoundsPack/7.ogg` | Exewin | https://opengameart.org/content/death-sounds-0 | CC0 | none |
| `assets/sounds/cc0/player_death.ogg` | Death sounds: `exewinDeathSoundsPack/6.ogg` | Exewin | https://opengameart.org/content/death-sounds-0 | CC0 | none |

All other sounds in `assets/sounds/synth/` are original, generated by `rr-tools synth`.

## Music

| Repo path | Original track and file | Author | Source | Licence | Processing |
|---|---|---|---|---|---|
| `assets/music/depot.ogg` | "Music loop, strong, downtempo, seamless": `space_ranger_seamless_loop.wav` | Nostromo | https://opengameart.org/content/music-loop-strong-downtempo-seamless | CC0 | full 72 s seamless loop, uncut; stereo OGG Vorbis q4, peak normalised to -1 dBFS (`CHANNELS=2 NO_TRIM=1`) |

## Models

Low-poly glTF models from poly.pizza, plus one OpenGameArt model; the licence (CC0 1.0) was
checked on each model's page. `scripts/import-models.sh` downloads them and checks each file's
pinned sha256; all are unmodified except the jetpack, converted from `.blend` as noted.
Scale, rotation, tint and attachment points are applied at runtime from `assets/defs/models.ron`.

| Repo path | Model | Author | Source | Licence | Processing |
|---|---|---|---|---|---|
| `assets/models/enemies/grunt.glb` | SWAT | Quaternius | https://poly.pizza/m/Btfn3G5Xv4 | CC0 | none |
| `assets/models/enemies/enforcer.glb` | Robot Enemy Large Gun | Quaternius | https://poly.pizza/m/mWojM4i2IH | CC0 | none |
| `assets/models/enemies/slasher.glb` | Alien | Quaternius | https://poly.pizza/m/HYUUkdugoP | CC0 | none |
| `assets/models/enemies/drone.glb` | Robot Enemy Flying Gun | Quaternius | https://poly.pizza/m/UDTM6X1y9a | CC0 | none |
| `assets/models/enemies/barrel.glb` | Exploding Barrel | Quaternius | https://poly.pizza/m/1orHe0kCc1 | CC0 | none |
| `assets/models/weapons/pistol.glb` | Pistol | Quaternius | https://poly.pizza/m/J3i9KDQ3kt | CC0 | none |
| `assets/models/weapons/shotgun.glb` | Shotgun Sawed Off | Quaternius | https://poly.pizza/m/29FXKu7G91 | CC0 | none |
| `assets/models/weapons/chaingun.glb` | Assault Rifle | Quaternius | https://poly.pizza/m/Bgvuu4CUMV | CC0 | none |
| `assets/models/weapons/rocket_launcher.glb` | Bazooka | CreativeTrio | https://poly.pizza/m/eJNzLpBsEt | CC0 | none |
| `assets/models/weapons/pipe_bomb.glb` | Hand Grenade | CreativeTrio | https://poly.pizza/m/YWhHlmKOtx | CC0 | none |
| `assets/models/weapons/detonator.glb` | Radio | Quaternius | https://poly.pizza/m/TPqvwkyWdV | CC0 | none |
| `assets/models/pickups/pistol_ammo.glb` | Pickup Crate | Quaternius | https://poly.pizza/m/1dh0EFL5gl | CC0 | none |
| `assets/models/pickups/shotgun_shells.glb` | Ammo Shotgun | CreativeTrio | https://poly.pizza/m/btpe2U9ODn | CC0 | none |
| `assets/models/pickups/rockets.glb` | Ammo Grenade Launcher | CreativeTrio | https://poly.pizza/m/BPdc4pA3tv | CC0 | none |
| `assets/models/pickups/health_small.glb` | Pickup Health | Quaternius | https://poly.pizza/m/cPLNFLykkf | CC0 | none |
| `assets/models/pickups/medkit.glb` | First Aid Kit | Quaternius | https://poly.pizza/m/Hp80p6148W | CC0 | none |
| `assets/models/pickups/armour.glb` | Armor Metal | Quaternius | https://poly.pizza/m/TMUoxILh9w | CC0 | none |
| `assets/models/pickups/keycard.glb` | Pickup Key Card | Quaternius | https://poly.pizza/m/EDvCEBvs8k | CC0 | none |
| `assets/models/pickups/atom.glb` | Pickup Sphere | Quaternius | https://poly.pizza/m/1xtAc12dmv | CC0 | none |
| `assets/models/pickups/night_vision.glb` | Binoculars | Voxel_dev | https://poly.pizza/m/PLmfHOiB08 | CC0 | none |
| `assets/models/pickups/jetpack.glb` | Lowpoly Jetpack: `Jetpacks.blend`, collection "Collection 1" | snabisch | https://opengameart.org/content/lowpoly-jetpack | CC0 | converted to glTF with Blender by `scripts/blend_to_glb.py` (Diffuse materials rebuilt as Principled in the same colours) |

## Fonts

The UI font (CC0 1.0, public domain; the pack's `License.txt` says so).

| Repo path | Original font and file | Author | Source | Licence | Processing |
|---|---|---|---|---|---|
| `assets/fonts/kenney_future.ttf` | Kenney Fonts: Kenney Future.ttf | Kenney | https://kenney.nl/assets/kenney-fonts | CC0 | none (sha256 bd36e536cc26672bdba652547494a4877f2a446329bb7141840d9cd73bceffc2) |

## Voice

Rex's spoken one-liners (`assets/quips/*.ogg`) are synthesised text-to-speech.

- **Voice model:** Kokoro-82M, voice `am_onyx`, https://huggingface.co/hexgrad/Kokoro-82M (pinned revision `f3ff3571791e39611d31c381e3a41a3af07b4987`). Model weights licensed Apache-2.0.
- **Training data (per the model card):** public-domain audio, Apache/MIT-licensed audio, two small CC-BY sets (Koniwa, CC BY 3.0; SIWIS, CC BY 4.0) and synthetic audio generated by closed TTS models from large providers. This is weaker provenance than the project's CC0/public-domain rule for shipped assets; it was accepted deliberately for the generated voice lines (the model is not shipped, only its output).
- **Quip texts** (`assets/quips/quips.ron`) are original to this project.
- **Generation:** the audio was generated offline by `scripts/gen-quips.sh` (Kokoro via `scripts/quips_kokoro.py`, then peak normalisation with sox). The model itself is not shipped.
