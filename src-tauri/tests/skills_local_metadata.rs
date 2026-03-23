mod support;

use rusqlite::params;
use support::SkillTestFixture;

const SOURCE_METADATA_FILE: &str = ".aio-coding-hub.source.json";

#[test]
fn skills_local_list_fails_when_source_metadata_is_invalid() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex Local Metadata List");

    let local_dir = fix.cli_skills_root.join("broken-metadata");
    std::fs::create_dir_all(&local_dir).expect("create local skill dir");
    std::fs::write(local_dir.join("SKILL.md"), "name: Broken Metadata Skill\n")
        .expect("write local skill md");
    std::fs::write(local_dir.join(SOURCE_METADATA_FILE), b"{invalid json")
        .expect("write invalid source metadata");

    let err = aio_coding_hub_lib::test_support::skills_local_list_json(&handle, fix.workspace_id)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("failed to parse source metadata"),
        "unexpected error: {err}"
    );
}

#[test]
fn skill_import_local_fails_when_source_metadata_is_invalid() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex Local Metadata Import");

    let local_dir = fix.cli_skills_root.join("broken-metadata");
    std::fs::create_dir_all(&local_dir).expect("create local skill dir");
    std::fs::write(local_dir.join("SKILL.md"), "name: Broken Metadata Skill\n")
        .expect("write local skill md");
    std::fs::write(local_dir.join(SOURCE_METADATA_FILE), b"{invalid json")
        .expect("write invalid source metadata");

    let err = aio_coding_hub_lib::test_support::skill_import_local_json(
        &handle,
        fix.workspace_id,
        "broken-metadata",
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("failed to parse source metadata"),
        "unexpected error: {err}"
    );

    let imported_count: i64 = fix
        .conn
        .query_row(
            "SELECT COUNT(1) FROM skills WHERE skill_key = ?1",
            params!["broken-metadata"],
            |row| row.get(0),
        )
        .expect("count imported skills");
    assert_eq!(
        imported_count, 0,
        "invalid metadata skill should not be imported"
    );
    assert!(
        !local_dir.join(".aio-coding-hub.managed").exists(),
        "import should not mark the local dir as managed"
    );
}
