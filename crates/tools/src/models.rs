//! Integrity and provenance checks for the glTF models under `assets/`: `defs/models.ron`
//! parses and covers everything, every scene it names exists and is well-formed, clip and node
//! names resolve, the triangle budget holds, and every `.glb` is referenced and credited.

use rr_core::models::{ModelDefs, Tip};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Most triangles one model file may have.
pub const MAX_TRIANGLES: usize = 12_000;

/// What the checks need to know about one glTF file.
struct SceneInfo {
    nodes: BTreeSet<String>,
    clips: BTreeSet<String>,
    triangles: usize,
}

/// Checks the models under `assets` (the directory holding `defs/models.ron` and `models/`).
/// Returns the printable report (one `label: error: ...` line per problem, then `ok` or
/// `FAILED`) and whether it passed.
pub fn validate_models(assets: &Path) -> (String, bool) {
    let label = format!("{}/models", assets.display());
    let mut errors = Vec::new();
    let defs = load_defs(assets, &mut errors);
    let mut referenced = BTreeSet::new();
    if let Some(defs) = &defs {
        referenced = check_scenes(assets, defs, &mut errors);
    }
    check_provenance(assets, defs.is_some(), &referenced, &mut errors);
    let mut out = String::new();
    for e in &errors {
        writeln!(out, "{label}: error: {e}").unwrap();
    }
    let ok = errors.is_empty();
    writeln!(out, "{label}: {}", if ok { "ok" } else { "FAILED" }).unwrap();
    (out, ok)
}

fn load_defs(assets: &Path, errors: &mut Vec<String>) -> Option<ModelDefs> {
    let path = assets.join("defs/models.ron");
    let defs = match std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read: {e}"))
        .and_then(|src| ModelDefs::from_ron(&src).map_err(|e| e.to_string()))
    {
        Ok(d) => d,
        Err(e) => {
            errors.push(format!("defs: defs/models.ron: {e}"));
            return None;
        }
    };
    if let Err(e) = defs.validate() {
        errors.push(format!("defs: {e}"));
    }
    Some(defs)
}

fn read_scene(path: &Path) -> Result<SceneInfo, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("not valid glTF: {e}"))?;
    let doc = &gltf.document;
    let mut triangles = 0;
    for mesh in doc.meshes() {
        for prim in mesh.primitives() {
            let count = match prim.indices() {
                Some(a) => a.count(),
                None => prim
                    .get(&gltf::Semantic::Positions)
                    .map_or(0, |a| a.count()),
            };
            triangles += count / 3;
        }
    }
    Ok(SceneInfo {
        nodes: doc
            .nodes()
            .filter_map(|n| n.name().map(str::to_string))
            .collect(),
        clips: doc
            .animations()
            .filter_map(|a| a.name().map(str::to_string))
            .collect(),
        triangles,
    })
}

/// Checks every scene `defs` names; returns the set of referenced scene paths.
fn check_scenes(assets: &Path, defs: &ModelDefs, errors: &mut Vec<String>) -> BTreeSet<String> {
    let mut cache: BTreeMap<String, Result<SceneInfo, String>> = BTreeMap::new();
    let mut load = |scene: &str, errors: &mut Vec<String>| -> bool {
        let entry = cache.entry(scene.to_string()).or_insert_with(|| {
            read_scene(&assets.join(scene)).and_then(|info| {
                if info.triangles > MAX_TRIANGLES {
                    Err(format!(
                        "{} triangles, budget is {MAX_TRIANGLES}",
                        info.triangles
                    ))
                } else {
                    Ok(info)
                }
            })
        });
        if let Err(e) = entry {
            errors.push(format!("scene: {scene}: {e}"));
        }
        entry.is_ok()
    };
    // Whether the first failure for a file has been reported; later users of a bad file stay quiet.
    let mut reported = BTreeSet::new();
    let mut refs: Vec<(String, &str)> = Vec::new();
    for e in &defs.enemies {
        refs.push((format!("enemy {:?}", e.kind), &e.scene));
        if let Some(a) = &e.attach {
            refs.push((format!("enemy {:?} attach", e.kind), &a.scene));
        }
    }
    for w in &defs.weapons {
        refs.push((format!("weapon {:?}", w.id), &w.scene));
    }
    for i in &defs.items {
        refs.push((format!("item {:?}", i.kind), &i.scene));
    }
    refs.push(("extras held_bomb".into(), &defs.extras.held_bomb.scene));
    refs.push(("extras detonator".into(), &defs.extras.detonator.scene));
    let mut referenced = BTreeSet::new();
    for (_, scene) in &refs {
        referenced.insert(scene.to_string());
        if reported.insert(scene.to_string()) {
            load(scene, errors);
        }
    }
    for e in &defs.enemies {
        let what = format!("enemy {:?}", e.kind);
        if let Some(Ok(info)) = cache.get(&e.scene) {
            let c = &e.clips;
            let named = [
                ("idle", &c.idle),
                ("walk", &c.walk),
                ("run", &c.run),
                ("attack", &c.attack),
                ("pain", &c.pain),
                ("death", &c.death),
            ];
            for (slot, clip) in named {
                if let Some(name) = clip
                    && !info.clips.contains(name)
                {
                    errors.push(format!(
                        "clips: {what}: {slot} clip {name:?} is not in {}",
                        e.scene
                    ));
                }
            }
            if let Tip::Node(n) = &e.tip
                && !info.nodes.contains(n)
            {
                errors.push(format!(
                    "nodes: {what}: tip node {n:?} is not in {}",
                    e.scene
                ));
            }
            if let Some(a) = &e.attach
                && !info.nodes.contains(&a.bone)
            {
                errors.push(format!(
                    "nodes: {what}: attach bone {:?} is not in {}",
                    a.bone, e.scene
                ));
            }
        }
    }
    referenced
}

/// Every `.glb` under `models/` must be referenced by `models.ron` and named in CREDITS.md.
/// With no readable `models.ron` only the credits half is checked.
fn check_provenance(
    assets: &Path,
    defs_ok: bool,
    referenced: &BTreeSet<String>,
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
    for rel in glb_files(&assets.join("models"), "models", errors) {
        if defs_ok && !referenced.contains(&rel) {
            errors.push(format!("provenance: {rel} is not referenced by models.ron"));
        }
        if !credits.contains(&format!("assets/{rel}")) {
            errors.push(format!(
                "provenance: assets/{rel} is not named in CREDITS.md"
            ));
        }
    }
}

/// `.glb` files under `dir` (recursively) as `/`-separated paths starting with `prefix`, sorted.
fn glb_files(dir: &Path, prefix: &str, errors: &mut Vec<String>) -> Vec<String> {
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
            let child = format!("{rel}/{}", e.file_name().to_string_lossy());
            let path = e.path();
            if path.is_dir() {
                stack.push((path, child));
            } else if path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("glb"))
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
    use rr_core::map::ActorKind;
    use rr_core::models::ItemKindModel;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

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
        fn check(&self) -> (String, bool) {
            validate_models(&self.assets())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A tiny glTF: nodes "Root" and "Hand", animation "Idle", one primitive of `indices`
    /// indices (so `indices / 3` triangles). The buffer is an empty base64 data URI; counts come
    /// from the accessors only.
    fn gltf(indices: usize) -> String {
        format!(
            r#"{{"asset":{{"version":"2.0"}},
"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"Root","mesh":0,"children":[1]}},{{"name":"Hand"}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"animations":[{{"name":"Idle","channels":[{{"sampler":0,"target":{{"node":1,"path":"translation"}}}}],
  "samplers":[{{"input":2,"output":3}}]}}],
"buffers":[{{"byteLength":4,"uri":"data:application/octet-stream;base64,AAAAAA=="}}],
"bufferViews":[{{"buffer":0,"byteLength":4}}],
"accessors":[
 {{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,1]}},
 {{"bufferView":0,"componentType":5123,"count":{indices},"type":"SCALAR"}},
 {{"bufferView":0,"componentType":5126,"count":1,"type":"SCALAR","min":[0],"max":[0]}},
 {{"bufferView":0,"componentType":5126,"count":1,"type":"VEC3"}}]}}"#
        )
    }

    fn scene(models: &mut String, f: &str) {
        write!(models, "scene: \"{f}\", ").unwrap();
    }

    /// Defs using `models/a.gltf` for everything; `tweak` edits the enemy fields.
    fn defs_ron(clip: &str, tip: &str, bone: &str) -> String {
        let mut s = String::from("(enemies: [");
        for k in ActorKind::ALL {
            s.push_str(&format!("(kind: {k:?}, "));
            scene(&mut s, "models/a.gltf");
            s.push_str(&format!(
                "clips: (idle: Some(\"{clip}\")), tip: {tip}, attach: Some((bone: \"{bone}\", scene: \"models/a.gltf\"))),"
            ));
        }
        s.push_str("], weapons: [");
        for w in rr_core::defs::WeaponId::ALL {
            if w != rr_core::defs::WeaponId::Boot {
                s.push_str(&format!("(id: {w:?}, scene: \"models/a.gltf\"),"));
            }
        }
        s.push_str("], items: [");
        for k in ItemKindModel::ALL {
            s.push_str(&format!("(kind: {k:?}, scene: \"models/a.gltf\"),"));
        }
        s.push_str(
            "], extras: (held_bomb: (scene: \"models/a.gltf\"), detonator: (scene: \"models/a.gltf\")))",
        );
        s
    }

    fn good() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "rr-models-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let f = Fixture { root };
        f.write(
            "defs/models.ron",
            &defs_ron("Idle", "Node(\"Hand\")", "Hand"),
        );
        f.write("models/a.gltf", &gltf(6));
        f.credits("assets/models/a.glb\n");
        f
    }

    fn fails_with(f: &Fixture, needle: &str) {
        let (report, ok) = f.check();
        assert!(!ok, "{report}");
        assert!(report.contains(needle), "{needle}: {report}");
        assert!(report.ends_with("FAILED\n"), "{report}");
    }

    // The fixture's only model is a .gltf, so provenance (which tracks .glb) has nothing to do.
    #[test]
    fn all_good() {
        let f = good();
        let (report, ok) = f.check();
        assert!(ok, "{report}");
        assert!(report.ends_with("models: ok\n"), "{report}");
    }

    #[test]
    fn missing_defs() {
        let f = good();
        std::fs::remove_file(f.assets().join("defs/models.ron")).unwrap();
        fails_with(&f, "defs/models.ron");
    }

    #[test]
    fn invalid_defs() {
        let f = good();
        f.write("defs/models.ron", "(enemies: [], weapons: [], items: [], extras: (held_bomb: (scene: \"a\"), detonator: (scene: \"a\")))");
        fails_with(&f, "no model");
    }

    #[test]
    fn missing_scene() {
        let f = good();
        std::fs::remove_file(f.assets().join("models/a.gltf")).unwrap();
        fails_with(&f, "scene: models/a.gltf: cannot read");
    }

    #[test]
    fn malformed_scene() {
        let f = good();
        f.write("models/a.gltf", "not gltf");
        fails_with(&f, "scene: models/a.gltf: not valid glTF");
    }

    #[test]
    fn missing_clip() {
        let f = good();
        f.write(
            "defs/models.ron",
            &defs_ron("Nope", "Node(\"Hand\")", "Hand"),
        );
        fails_with(&f, "idle clip \"Nope\"");
    }

    #[test]
    fn missing_tip_node() {
        let f = good();
        f.write(
            "defs/models.ron",
            &defs_ron("Idle", "Node(\"Nose\")", "Hand"),
        );
        fails_with(&f, "tip node \"Nose\"");
    }

    #[test]
    fn offset_tip_needs_no_node() {
        let f = good();
        f.write(
            "defs/models.ron",
            &defs_ron("Idle", "Offset((0.0, 1.0, 0.0))", "Hand"),
        );
        let (report, ok) = f.check();
        assert!(ok, "{report}");
    }

    #[test]
    fn missing_attach_bone() {
        let f = good();
        f.write(
            "defs/models.ron",
            &defs_ron("Idle", "Node(\"Hand\")", "Elbow"),
        );
        fails_with(&f, "attach bone \"Elbow\"");
    }

    #[test]
    fn triangle_budget() {
        let f = good();
        f.write("models/a.gltf", &gltf(3 * (MAX_TRIANGLES + 1)));
        fails_with(&f, "12001 triangles, budget is 12000");
        f.write("models/a.gltf", &gltf(3 * MAX_TRIANGLES));
        let (report, ok) = f.check();
        assert!(ok, "{report}");
    }

    #[test]
    fn unreferenced_glb() {
        let f = good();
        f.write("models/extra.glb", "x");
        f.credits("assets/models/extra.glb\n");
        fails_with(&f, "models/extra.glb is not referenced by models.ron");
    }

    #[test]
    fn uncredited_glb() {
        let f = good();
        f.write("models/extra.glb", "x");
        fails_with(&f, "assets/models/extra.glb is not named in CREDITS.md");
    }

    #[test]
    fn missing_credits() {
        let f = good();
        std::fs::remove_file(f.root.join("CREDITS.md")).unwrap();
        fails_with(&f, "cannot read CREDITS.md");
    }

    #[test]
    fn real_assets_pass() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let (report, ok) = validate_models(&assets);
        assert!(ok, "{report}");
    }
}
