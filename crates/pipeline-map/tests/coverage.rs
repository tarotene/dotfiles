//! リポジトリ本体の `pipeline-map.toml` を登録箇所と突き合わせる。
//! 自動発火する機構を足したら manifest にノードを足し、登録箇所にポインタを書く
//! (Claude hook は `node`、それ以外は `pipeline-node:`)。

use std::path::Path;

#[test]
fn repository_manifest_matches_registrations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m = pipeline_map::load(&root).expect("pipeline-map.toml");
    let r = pipeline_map::audit(&root, &m);
    eprintln!(
        "pipeline-map: node {}(auto {})/ edge {} / 登録箇所 {}",
        r.nodes, r.auto_nodes, r.edges, r.sites
    );
    assert!(
        r.problems.is_empty(),
        "pipeline-map.toml と登録箇所が食い違っています:\n  {}",
        r.problems.join("\n  ")
    );
}

#[test]
fn shared_skills_have_matching_names_and_single_deployment_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let module = std::fs::read_to_string(root.join("home/modules/agents.nix")).unwrap();
    let skills = root.join("config/agents/skills");
    for entry in std::fs::read_dir(&skills).unwrap().flatten() {
        let skill = entry.path().join("SKILL.md");
        if !skill.exists() {
            continue;
        } // reference-only repo-governance-common
        let text = std::fs::read_to_string(&skill).unwrap();
        let name = text
            .lines()
            .find_map(|l| l.strip_prefix("name: "))
            .expect("skill name");
        assert_eq!(
            name,
            entry.file_name().to_str().unwrap(),
            "{}",
            skill.display()
        );
        assert!(
            module.contains(&format!("/agents/skills/{name}/SKILL.md"))
                || module.contains(&format!("/agents/skills/{name}\"")),
            "undeployed skill {name}"
        );
        if let Some(tail) = text.split("Read `../").nth(1) {
            let target = tail.split('`').next().unwrap();
            assert!(
                skills.join(target).is_file(),
                "broken compatibility skill {name}"
            );
            assert_eq!(text.matches("Read `../").count(), 1);
        }
    }
    // Every declared literal source must exist, including supplemental references.
    for tail in module.split("repoConfig + \"").skip(1) {
        let source = tail.split('"').next().unwrap();
        assert!(
            root.join("config")
                .join(source.trim_start_matches('/'))
                .exists(),
            "missing source {source}"
        );
    }
    assert!(!module.contains("repoConfig + \"/claude/skills/"));
}

#[test]
fn public_pr_contract_crates_keep_their_dependency_free_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["pr-title-check", "pr-contract-check"] {
        let manifest =
            std::fs::read_to_string(root.join(format!("crates/{name}/Cargo.toml"))).unwrap();
        let parsed: toml::Value = toml::from_str(&manifest).unwrap();
        assert!(parsed
            .get("dependencies")
            .is_none_or(|v| v.as_table().unwrap().is_empty()));
    }
}
