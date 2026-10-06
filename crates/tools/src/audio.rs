//! Integrity and provenance checks for the audio content under `assets/`: the sound bank, the
//! hero quips, level music, and a credit for every audio file that is not generated here.

use crate::synth::parse_recipes;
use rr_core::audio::{QuipTable, SoundBankDef};
use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Checks the audio content under `assets` (the directory holding `sounds/`, `music/`, `quips/`).
/// `music` lists the track names the validated levels ask for. Returns the printable report
/// (one `label: error: ...` line per problem, then `ok` or `FAILED`) and whether it passed.
pub fn validate_audio(assets: &Path, music: &[String]) -> (String, bool) {
    let label = format!("{}/audio", assets.display());
    let mut errors = Vec::new();
    let recipes = check_recipes(assets, &mut errors);
    check_bank(assets, &mut errors);
    let quip_ids = check_quips(assets, &mut errors);
    for name in music.iter().collect::<BTreeSet<_>>() {
        if !assets.join("music").join(format!("{name}.ogg")).is_file() {
            errors.push(format!("music: music/{name}.ogg is missing"));
        }
    }
    check_provenance(assets, &recipes, &quip_ids, &mut errors);
    let mut out = String::new();
    for e in &errors {
        writeln!(out, "{label}: error: {e}").unwrap();
    }
    let ok = errors.is_empty();
    writeln!(out, "{label}: {}", if ok { "ok" } else { "FAILED" }).unwrap();
    (out, ok)
}

/// Recipe names from `sounds/synth.ron`; a missing or broken file is an error.
fn check_recipes(assets: &Path, errors: &mut Vec<String>) -> BTreeSet<String> {
    let path = assets.join("sounds/synth.ron");
    match std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read: {e}"))
        .and_then(|src| parse_recipes(&src))
    {
        Ok(list) => list.into_iter().map(|r| r.name).collect(),
        Err(e) => {
            errors.push(format!("synth: sounds/synth.ron: {e}"));
            BTreeSet::new()
        }
    }
}

fn check_bank(assets: &Path, errors: &mut Vec<String>) {
    let path = assets.join("sounds/bank.ron");
    let bank = match std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read: {e}"))
        .and_then(|src| SoundBankDef::from_ron(&src).map_err(|e| e.to_string()))
    {
        Ok(b) => b,
        Err(e) => {
            errors.push(format!("bank: sounds/bank.ron: {e}"));
            return;
        }
    };
    errors.extend(bank.problems().into_iter().map(|p| format!("bank: {p}")));
    let mut defs: Vec<_> = bank.0.iter().collect();
    defs.sort_by_key(|(c, _)| format!("{c:?}"));
    for (cue, def) in defs {
        if def.volume > 2.0 {
            errors.push(format!("bank: {cue:?}: volume {} is above 2", def.volume));
        }
    }
    for file in bank.files() {
        if !assets.join(file).is_file() {
            errors.push(format!("bank: {file} is missing"));
        }
    }
}

/// Quip ids from `quips/quips.ron`, after checking each has its OGG and no OGG is stray.
fn check_quips(assets: &Path, errors: &mut Vec<String>) -> BTreeSet<String> {
    let path = assets.join("quips/quips.ron");
    let table = match std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read: {e}"))
        .and_then(|src| QuipTable::parse(&src).map_err(|e| e.to_string()))
    {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("quips: quips/quips.ron: {e}"));
            return BTreeSet::new();
        }
    };
    let ids: BTreeSet<String> = table.quips.into_iter().map(|q| q.id).collect();
    for id in &ids {
        if !assets.join("quips").join(format!("{id}.ogg")).is_file() {
            errors.push(format!("quips: quips/{id}.ogg is missing"));
        }
    }
    for rel in audio_files(&assets.join("quips"), "quips", errors) {
        let stem = rel
            .strip_prefix("quips/")
            .and_then(|f| f.strip_suffix(".ogg"));
        if !stem.is_some_and(|s| ids.contains(s)) {
            errors.push(format!("quips: {rel} has no id in quips.ron"));
        }
    }
    ids
}

/// Every audio file must be generated here (synth WAV with a recipe, quip OGG with an id) or be
/// named in CREDITS.md.
fn check_provenance(
    assets: &Path,
    recipes: &BTreeSet<String>,
    quip_ids: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    let credits = assets
        .parent()
        .map(|root| root.join("CREDITS.md"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    let Some(credits) = credits else {
        errors.push("provenance: cannot read CREDITS.md next to assets/".into());
        return;
    };
    if !credits.contains("scripts/gen-quips.sh") {
        errors.push("provenance: CREDITS.md does not mention scripts/gen-quips.sh".into());
    }
    for dir in ["sounds", "music", "quips"] {
        for rel in audio_files(&assets.join(dir), dir, errors) {
            let synth = rel
                .strip_prefix("sounds/synth/")
                .and_then(|f| f.strip_suffix(".wav"))
                .is_some_and(|n| recipes.contains(n));
            let quip = rel
                .strip_prefix("quips/")
                .and_then(|f| f.strip_suffix(".ogg"))
                .is_some_and(|n| quip_ids.contains(n));
            if !synth && !quip && !credits.contains(&rel) {
                errors.push(format!(
                    "provenance: {rel} is not a synth output, a quip, or named in CREDITS.md"
                ));
            }
        }
    }
}

/// `.wav`/`.ogg` files under `dir` (recursively), as `/`-separated paths starting with `prefix`,
/// sorted. Extensions match case-insensitively. A missing `dir` itself is fine; an unreadable
/// subdirectory is a validation error.
fn audio_files(dir: &Path, prefix: &str, errors: &mut Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String)> = vec![(dir.to_path_buf(), prefix.to_string())];
    while let Some((d, rel)) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(entries) => entries,
            Err(_) if d == dir => continue,
            Err(e) => {
                errors.push(format!("provenance: cannot read {rel}/: {e}"));
                continue;
            }
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let child = format!("{rel}/{name}");
            let path = e.path();
            if path.is_dir() {
                stack.push((path, child));
            } else if path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("wav") || x.eq_ignore_ascii_case("ogg"))
            {
                out.push(child);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    /// A minimal good tree: `<tmp>/assets` plus `<tmp>/CREDITS.md`.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn assets(&self) -> PathBuf {
            self.root.join("assets")
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.assets().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        fn credits(&self, body: &str) {
            std::fs::write(self.root.join("CREDITS.md"), body).unwrap();
        }
        fn check(&self, music: &[&str]) -> (String, bool) {
            let m: Vec<String> = music.iter().map(|s| s.to_string()).collect();
            validate_audio(&self.assets(), &m)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Every cue maps to `sounds/cc0/a.ogg` (credited), except one using the synth WAV.
    fn good() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "rr-audio-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let f = Fixture { root };
        let entries: String = rr_core::audio::Cue::all()
            .into_iter()
            .map(|c| format!("{c:?}: (files: [\"sounds/cc0/a.ogg\"]),\n"))
            .collect();
        f.write("sounds/bank.ron", &format!("{{\n{entries}}}"));
        f.write(
            "sounds/synth.ron",
            r#"[(name: "pistol", layers: [(wave: Sine, freq: 440.0, sustain: 0.1)])]"#,
        );
        f.write("sounds/synth/pistol.wav", "x");
        f.write("sounds/cc0/a.ogg", "x");
        f.write("music/depot.ogg", "x");
        f.write(
            "quips/quips.ron",
            r#"[(id: "hi", on: LevelStart, text: "Hi.")]"#,
        );
        f.write("quips/hi.ogg", "x");
        f.credits("sounds/cc0/a.ogg\nmusic/depot.ogg\nscripts/gen-quips.sh\n");
        f
    }

    fn fails_with(f: &Fixture, music: &[&str], needle: &str) {
        let (report, ok) = f.check(music);
        assert!(!ok, "{report}");
        assert!(report.contains(needle), "{needle}: {report}");
        assert!(report.ends_with("FAILED\n"), "{report}");
    }

    #[test]
    fn all_good() {
        let f = good();
        let (report, ok) = f.check(&["depot"]);
        assert!(ok, "{report}");
        assert!(report.ends_with("audio: ok\n"), "{report}");
    }

    #[test]
    fn missing_bank_entry() {
        let f = good();
        let src = std::fs::read_to_string(f.assets().join("sounds/bank.ron")).unwrap();
        let first = src.lines().nth(1).unwrap().to_string();
        f.write(
            "sounds/bank.ron",
            &src.replacen(&format!("{first}\n"), "", 1),
        );
        fails_with(&f, &[], "no entry");
    }

    #[test]
    fn missing_bank_file() {
        let f = good();
        std::fs::remove_file(f.assets().join("sounds/cc0/a.ogg")).unwrap();
        fails_with(&f, &[], "sounds/cc0/a.ogg is missing");
    }

    #[test]
    fn bad_ref_dist() {
        let f = good();
        let src = std::fs::read_to_string(f.assets().join("sounds/bank.ron")).unwrap();
        f.write(
            "sounds/bank.ron",
            &src.replacen("(files:", "(ref_dist: 50.0, files:", 1),
        );
        fails_with(&f, &[], "ref_dist");
    }

    #[test]
    fn missing_quip_ogg() {
        let f = good();
        std::fs::remove_file(f.assets().join("quips/hi.ogg")).unwrap();
        fails_with(&f, &[], "quips/hi.ogg is missing");
    }

    #[test]
    fn stray_quip_ogg() {
        let f = good();
        f.write("quips/stray.ogg", "x");
        fails_with(&f, &[], "quips/stray.ogg has no id");
    }

    #[test]
    fn missing_music() {
        let f = good();
        fails_with(&f, &["nope"], "music/nope.ogg is missing");
    }

    #[test]
    fn uncredited_cc0_file() {
        let f = good();
        f.write("sounds/cc0/b.ogg", "x");
        fails_with(&f, &[], "sounds/cc0/b.ogg is not a synth output");
    }

    #[test]
    fn uncredited_uppercase_extension() {
        let f = good();
        f.write("sounds/cc0/LOUD.OGG", "x");
        fails_with(&f, &[], "sounds/cc0/LOUD.OGG is not a synth output");
    }

    #[test]
    fn synth_wav_without_recipe() {
        let f = good();
        f.write("sounds/synth/orphan.wav", "x");
        fails_with(&f, &[], "sounds/synth/orphan.wav is not a synth output");
    }

    #[test]
    fn credits_must_mention_quip_script() {
        let f = good();
        f.credits("sounds/cc0/a.ogg\nmusic/depot.ogg\n");
        fails_with(&f, &[], "scripts/gen-quips.sh");
    }

    #[test]
    fn real_assets_pass() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let levels = std::fs::read_dir(assets.join("levels")).unwrap();
        let music: Vec<String> = levels
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|s| rr_core::map::Map::from_ron(&s).ok())
            .filter_map(|m| m.music)
            .collect();
        let (report, ok) = validate_audio(&assets, &music);
        assert!(ok, "{report}");
    }
}
