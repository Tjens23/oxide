use crate::{
    commands::{
        command_handler::CommandHandler, dlx::DlxHandler, doctor::DoctorHandler,
        foreach::ForeachHandler, install::InstallHandler, link::LinkHandler, ls::LsHandler,
        outdated::OutdatedHandler, pack::PackHandler, publish::PublishHandler, run::RunHandler,
        unlink::UnlinkHandler, why::WhyHandler, workspaces::WorkspacesHandler,
    },
    versions::Versions,
    workspace,
};

fn args(v: &[&str]) -> impl Iterator<Item = String> {
    v.iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

#[test]
fn test_stringify() {
    assert_eq!(
        Versions::stringify(&"lodash".to_string(), &"4.17.21".to_string()),
        "lodash@4.17.21"
    );
}

#[test]
fn test_parse_raw_package_details() {
    let (name, version) = Versions::parse_raw_package_details("lodash@4.17.21".to_string());
    assert_eq!(name, "lodash");
    assert_eq!(version, "4.17.21");
}

#[test]
fn test_parse_raw_no_version() {
    let (name, version) = Versions::parse_raw_package_details("lodash".to_string());
    assert_eq!(name, "lodash");
    assert_eq!(version, "");
}

#[test]
fn test_parse_raw_scoped_package() {
    let (name, version) = Versions::parse_raw_package_details("@myorg/ui@1.2.3".to_string());
    assert_eq!(name, "@myorg/ui");
    assert_eq!(version, "1.2.3");
}

#[test]
fn test_link_parse() {
    let mut h = LinkHandler::default();
    h.parse(&mut args(&["lodash"])).unwrap();
}

#[test]
fn test_unlink_parse() {
    let mut h = UnlinkHandler::default();
    h.parse(&mut args(&["lodash"])).unwrap();
}

#[test]
fn test_publish_parse() {
    let mut h = PublishHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_outdated_parse() {
    let mut h = OutdatedHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_ls_parse_no_flags() {
    let mut h = LsHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_pack_parse_no_flags() {
    let mut h = PackHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_doctor_parse() {
    let mut h = DoctorHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_dlx_parse_package() {
    let mut h = DlxHandler::default();
    h.parse(&mut args(&["typescript"])).unwrap();
}

#[test]
fn test_dlx_parse_missing_package_errors() {
    let mut h = DlxHandler::default();
    assert!(h.parse(&mut args(&[])).is_err());
}

#[test]
fn test_resolve_binary_string_bin_field() {
    use crate::commands::dlx::resolve_binary;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-string-bin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"tsconfig","version":"1.0.0","bin":"dist/index.js"}"#,
    )
    .unwrap();
    let result = resolve_binary(dir.to_str().unwrap(), "tsconfig", None).unwrap();
    assert_eq!(result, format!("{}/dist/index.js", dir.to_str().unwrap()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_resolve_binary_object_bin_field_matching_key() {
    use crate::commands::dlx::resolve_binary;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-obj-bin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"tsconfig","version":"1.0.0","bin":{"tsconfig":"dist/index.js"}}"#,
    )
    .unwrap();
    let result = resolve_binary(dir.to_str().unwrap(), "tsconfig", None).unwrap();
    assert_eq!(result, format!("{}/dist/index.js", dir.to_str().unwrap()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_resolve_binary_object_bin_field_binary_override() {
    use crate::commands::dlx::resolve_binary;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-bin-override-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"mypkg","version":"1.0.0","bin":{"cli":"dist/cli.js","other":"dist/other.js"}}"#,
    )
    .unwrap();
    let result = resolve_binary(dir.to_str().unwrap(), "mypkg", Some("other")).unwrap();
    assert_eq!(result, format!("{}/dist/other.js", dir.to_str().unwrap()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_resolve_binary_no_bin_field_errors() {
    use crate::commands::dlx::resolve_binary;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-no-bin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"mypkg","version":"1.0.0"}"#,
    )
    .unwrap();
    assert!(resolve_binary(dir.to_str().unwrap(), "mypkg", None).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_js_extension_detection() {
    for ext in &[".js", ".cjs", ".mjs"] {
        let path = format!("dist/index{}", ext);
        let is_js = path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".mjs");
        assert!(is_js, "{} should be detected as JS", path);
    }
}

#[test]
fn test_non_js_extension_not_detected() {
    for path in &["dist/index", "bin/tool", "dist/index.exe", "dist/index.sh"] {
        let is_js = path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".mjs");
        assert!(!is_js, "{} should NOT be detected as JS", path);
    }
}

#[test]
fn test_parse_shebang_env_node() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(
        parse_shebang("#!/usr/bin/env node"),
        Some("node".to_string())
    );
}

#[test]
fn test_parse_shebang_env_bun() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(parse_shebang("#!/usr/bin/env bun"), Some("bun".to_string()));
}

#[test]
fn test_parse_shebang_env_deno() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(
        parse_shebang("#!/usr/bin/env deno"),
        Some("deno".to_string())
    );
}

#[test]
fn test_parse_shebang_absolute_path() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(parse_shebang("#!/usr/bin/node"), Some("node".to_string()));
}

#[test]
fn test_parse_shebang_no_shebang() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(parse_shebang("console.log('hello')"), None);
}

#[test]
fn test_parse_shebang_empty() {
    use crate::commands::dlx::parse_shebang;
    assert_eq!(parse_shebang(""), None);
}

#[test]
fn test_resolve_interpreter_uses_shebang() {
    use crate::commands::dlx::resolve_interpreter;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-shebang-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("index.js");
    std::fs::write(&script, "#!/usr/bin/env bun\nconsole.log('hi')").unwrap();
    assert_eq!(resolve_interpreter(script.to_str().unwrap()), "bun");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_resolve_interpreter_falls_back_to_node() {
    use crate::commands::dlx::resolve_interpreter;
    let dir = std::env::temp_dir().join(format!(
        "oxide-dlx-test-noshebang-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("index.js");
    std::fs::write(&script, "console.log('hi')").unwrap();
    assert_eq!(resolve_interpreter(script.to_str().unwrap()), "node");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_resolve_interpreter_missing_file_falls_back_to_node() {
    use crate::commands::dlx::resolve_interpreter;
    assert_eq!(resolve_interpreter("/nonexistent/path/index.js"), "node");
}

#[test]
fn test_why_parse() {
    let mut h = WhyHandler::default();
    h.parse(&mut args(&["lodash"])).unwrap();
}

#[test]
fn test_why_parse_missing_arg_errors() {
    let mut h = WhyHandler::default();
    assert!(h.parse(&mut args(&[])).is_err());
}

#[test]
fn test_ls_parse_all_flag() {
    let mut h = LsHandler::default();
    h.parse(&mut args(&["--all"])).unwrap();
}

#[test]
fn test_ls_parse_dev_flag() {
    let mut h = LsHandler::default();
    h.parse(&mut args(&["--dev"])).unwrap();
}

#[test]
fn test_pack_parse_out_dir() {
    let mut h = PackHandler::default();
    h.parse(&mut args(&["--out-dir", "/tmp"])).unwrap();
}

#[test]
fn test_pack_parse_missing_out_dir_value_errors() {
    let mut h = PackHandler::default();
    assert!(h.parse(&mut args(&["--out-dir"])).is_err());
}

#[test]
fn test_install_parse_plain() {
    let mut h = InstallHandler::default();
    h.parse(&mut args(&["lodash"])).unwrap();
}

#[test]
fn test_install_parse_with_filter() {
    let mut h = InstallHandler::default();
    h.parse(&mut args(&["--filter", "@myorg/ui", "lodash"]))
        .unwrap();
}

#[test]
fn test_install_parse_filter_shorthand() {
    let mut h = InstallHandler::default();
    h.parse(&mut args(&["-F", "api", "express"])).unwrap();
}

#[test]
fn test_run_parse_plain_script() {
    let mut h = RunHandler::default();
    h.parse(&mut args(&["test"])).unwrap();
}

#[test]
fn test_run_parse_filter() {
    let mut h = RunHandler::default();
    h.parse(&mut args(&["test", "--filter", "@myorg/ui"]))
        .unwrap();
}

#[test]
fn test_run_parse_recursive() {
    let mut h = RunHandler::default();
    h.parse(&mut args(&["-r", "build"])).unwrap();
}

#[test]
fn test_run_parse_recursive_long() {
    let mut h = RunHandler::default();
    h.parse(&mut args(&["--recursive", "lint"])).unwrap();
}

#[test]
fn test_run_parse_filter_shorthand() {
    let mut h = RunHandler::default();
    h.parse(&mut args(&["-F", "api", "test"])).unwrap();
}

#[test]
fn test_workspaces_parse_no_args() {
    let mut h = WorkspacesHandler::default();
    h.parse(&mut args(&[])).unwrap();
}

#[test]
fn test_workspaces_parse_filter() {
    let mut h = WorkspacesHandler::default();
    h.parse(&mut args(&["--filter", "@myorg/*"])).unwrap();
}

#[test]
fn test_workspaces_parse_filter_missing_value_errors() {
    let mut h = WorkspacesHandler::default();
    assert!(h.parse(&mut args(&["--filter"])).is_err());
}

#[test]
fn test_foreach_parse_bare_script() {
    let mut h = ForeachHandler::default();
    h.parse(&mut args(&["test"])).unwrap();
}

#[test]
fn test_foreach_parse_run_prefix() {
    let mut h = ForeachHandler::default();
    h.parse(&mut args(&["run", "build"])).unwrap();
}

#[test]
fn test_foreach_parse_filter() {
    let mut h = ForeachHandler::default();
    h.parse(&mut args(&["test", "--filter", "@myorg/ui"]))
        .unwrap();
}

#[test]
fn test_foreach_parse_bail_flag() {
    let mut h = ForeachHandler::default();
    h.parse(&mut args(&["build", "--bail"])).unwrap();
}

#[test]
fn test_foreach_parse_double_dash_passthrough() {
    let mut h = ForeachHandler::default();
    h.parse(&mut args(&["test", "--", "--watch"])).unwrap();
}

#[test]
fn test_foreach_parse_missing_script_errors() {
    let mut h = ForeachHandler::default();
    assert!(h.parse(&mut args(&[])).is_err());
}

#[test]
fn test_foreach_run_prefix_missing_script_errors() {
    let mut h = ForeachHandler::default();
    assert!(h.parse(&mut args(&["run"])).is_err());
}

fn ws_pkg(name: &str, path: &str) -> workspace::WorkspacePackage {
    workspace::WorkspacePackage {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        path: std::path::PathBuf::from(path),
        scripts: vec![],
    }
}

#[test]
fn test_workspace_filter_exact_name() {
    let pkgs = vec![
        ws_pkg("@myorg/ui", "packages/ui"),
        ws_pkg("@myorg/api", "packages/api"),
    ];
    let matched = workspace::apply_filter(&pkgs, "@myorg/ui");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "@myorg/ui");
}

#[test]
fn test_workspace_filter_scope_wildcard() {
    let pkgs = vec![
        ws_pkg("@myorg/ui", "packages/ui"),
        ws_pkg("@myorg/api", "packages/api"),
        ws_pkg("unrelated", "apps/cli"),
    ];
    let matched = workspace::apply_filter(&pkgs, "@myorg/*");
    assert_eq!(matched.len(), 2);
}

#[test]
fn test_workspace_filter_path_substring() {
    let pkgs = vec![ws_pkg("ui", "packages/ui"), ws_pkg("cli", "apps/cli")];
    let matched = workspace::apply_filter(&pkgs, "apps");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "cli");
}

#[test]
fn test_workspace_filter_trailing_ellipsis() {
    let pkgs = vec![ws_pkg("@myorg/ui", "packages/ui")];
    let matched = workspace::apply_filter(&pkgs, "@myorg/ui...");
    assert_eq!(matched.len(), 1);
}

#[test]
fn test_workspace_filter_no_match() {
    let pkgs = vec![ws_pkg("@myorg/ui", "packages/ui")];
    assert!(workspace::apply_filter(&pkgs, "nonexistent").is_empty());
}

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "oxide-test-{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ))
}

#[test]
fn test_workspace_discover_glob() {
    let root = tmp_dir("discover");
    std::fs::create_dir_all(root.join("packages/alpha")).unwrap();
    std::fs::create_dir_all(root.join("packages/beta")).unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\":\"root\",\"workspaces\":[\"packages/*\"]}",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/alpha/package.json"),
        "{\"name\":\"alpha\",\"version\":\"1.0.0\"}",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/beta/package.json"),
        "{\"name\":\"beta\",\"version\":\"2.0.0\"}",
    )
    .unwrap();

    let pkgs = workspace::discover(&root).unwrap();
    assert_eq!(pkgs.len(), 2);
    let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_workspace_discover_no_workspaces_field_errors() {
    let root = tmp_dir("nofield");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("package.json"), "{\"name\":\"root\"}").unwrap();

    assert!(workspace::discover(&root).is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_workspace_discover_yarn_object_format() {
    let root = tmp_dir("yarn");
    std::fs::create_dir_all(root.join("pkgs/core")).unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\":\"root\",\"workspaces\":{\"packages\":[\"pkgs/*\"]}}",
    )
    .unwrap();
    std::fs::write(
        root.join("pkgs/core/package.json"),
        "{\"name\":\"core\",\"version\":\"0.1.0\"}",
    )
    .unwrap();

    let pkgs = workspace::discover(&root).unwrap();
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].name, "core");

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_workspace_discover_sorted_by_name() {
    let root = tmp_dir("sorted");
    std::fs::create_dir_all(root.join("packages/z-pkg")).unwrap();
    std::fs::create_dir_all(root.join("packages/a-pkg")).unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\":\"root\",\"workspaces\":[\"packages/*\"]}",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/z-pkg/package.json"),
        "{\"name\":\"z-pkg\",\"version\":\"1.0.0\"}",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a-pkg/package.json"),
        "{\"name\":\"a-pkg\",\"version\":\"1.0.0\"}",
    )
    .unwrap();

    let pkgs = workspace::discover(&root).unwrap();
    assert_eq!(pkgs[0].name, "a-pkg");
    assert_eq!(pkgs[1].name, "z-pkg");

    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(test)]
mod integrity_tests {
    use crate::util::verify_integrity;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use bytes::Bytes;
    use sha2::{Digest, Sha512};

    fn make_integrity(data: &[u8]) -> String {
        let digest = Sha512::digest(data);
        format!("sha512-{}", BASE64.encode(digest.as_slice()))
    }

    #[test]
    fn test_valid_integrity() {
        let data = b"hello world";
        let bytes = Bytes::from_static(data);
        let integrity = make_integrity(data);
        assert!(verify_integrity(&bytes, &integrity));
    }

    #[test]
    fn test_tampered_bytes_fail() {
        let data = b"hello world";
        let tampered = Bytes::from_static(b"hello world!"); // one extra char
        let integrity = make_integrity(data);
        assert!(!verify_integrity(&tampered, &integrity));
    }

    #[test]
    fn test_empty_bytes() {
        let data = b"";
        let bytes = Bytes::from_static(data);
        let integrity = make_integrity(data);
        assert!(verify_integrity(&bytes, &integrity));
    }

    #[test]
    fn test_unknown_algorithm_rejected() {
        let bytes = Bytes::from_static(b"hello");
        // sha256 prefix — not supported, should return false
        assert!(!verify_integrity(&bytes, "sha256-abc123"));
    }

    #[test]
    fn test_malformed_integrity_string() {
        let bytes = Bytes::from_static(b"hello");
        assert!(!verify_integrity(&bytes, "not-an-integrity-string"));
        assert!(!verify_integrity(&bytes, "sha512-")); // empty digest
        assert!(!verify_integrity(&bytes, "sha512-!!!!!!")); // invalid base64
    }
}
