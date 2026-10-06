# Credits

All code and content in this repository are original. Milestone M1 uses no
third-party assets: textures are generated procedurally at runtime. Milestone M3
adds none either: weapons, enemies and effects are built from Bevy primitives at
runtime.

Third-party CC0 assets added in later milestones are listed here with their
source URL and licence.

All third-party files below are CC0 1.0 (public domain dedication,
https://creativecommons.org/publicdomain/zero/1.0/). Each was converted with
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

## Voice

Rex's spoken one-liners (`assets/quips/*.ogg`) are synthesised text-to-speech.

- **Voice model:** `en_US-norman-medium` from rhasspy/piper-voices, https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/norman/medium (model card: https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/norman/medium/MODEL_CARD). Trained from scratch on LibriVox recordings, which are public domain; no attribution is required, credited here as good practice.
- **Engine:** Piper (rhasspy/piper, MIT; successor OHF-Voice/piper1-gpl, GPL-3.0). The engine licence covers the program, not the audio it synthesises.
- **Quip texts** (`assets/quips/quips.ron`) are original to this project.
- **Generation:** the audio was generated offline by `scripts/gen-quips.sh` (Piper plus a sox gruff-hero effect chain). The model itself is not shipped.
