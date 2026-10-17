use super::*;

#[test]
fn collect_assets_requires_minisign_signature_for_each_dmg() {
    let dist = tempfile::tempdir().unwrap();
    fs_err::write(dist.path().join("OpenLogi-v1.2.3-macos-arm64.dmg"), b"dmg").unwrap();

    collect_assets(
        dist.path(),
        "https://updates.example/releases/v1.2.3",
        false,
    )
    .unwrap_err();
}

#[test]
fn collect_assets_publishes_signature_url() {
    let dist = tempfile::tempdir().unwrap();
    fs_err::write(dist.path().join("OpenLogi-v1.2.3-macos-arm64.dmg"), b"dmg").unwrap();
    fs_err::write(
        dist.path().join("OpenLogi-v1.2.3-macos-arm64.dmg.minisig"),
        b"signature",
    )
    .unwrap();

    let assets = collect_assets(
        dist.path(),
        "https://updates.example/releases/v1.2.3",
        false,
    )
    .unwrap();

    assert_eq!(
        assets[0].signature_url,
        "https://updates.example/releases/v1.2.3/OpenLogi-v1.2.3-macos-arm64.dmg.minisig"
    );
}

#[test]
fn collect_assets_skips_windows_artifacts_unless_opted_in() {
    // Off by default: the manifest must never reference Windows objects
    // the release workflow's R2 upload step doesn't ship.
    let dist = tempfile::tempdir().unwrap();
    for name in [
        "OpenLogi-v1.2.3-windows-x86_64.msi",
        "OpenLogi-v1.2.3-windows-x86_64.zip",
    ] {
        fs_err::write(dist.path().join(name), b"artifact").unwrap();
        fs_err::write(dist.path().join(format!("{name}.minisig")), b"signature").unwrap();
    }

    let assets = collect_assets(
        dist.path(),
        "https://updates.example/releases/v1.2.3",
        false,
    )
    .unwrap();

    assert!(assets.is_empty());
}

#[test]
fn collect_assets_includes_windows_msi_and_zip_per_arch() {
    let dist = tempfile::tempdir().unwrap();
    for name in [
        "OpenLogi-v1.2.3-windows-x86_64.msi",
        "OpenLogi-v1.2.3-windows-arm64.msi",
        "OpenLogi-v1.2.3-windows-x86_64.zip",
    ] {
        fs_err::write(dist.path().join(name), b"artifact").unwrap();
        fs_err::write(dist.path().join(format!("{name}.minisig")), b"signature").unwrap();
    }

    let assets =
        collect_assets(dist.path(), "https://updates.example/releases/v1.2.3", true).unwrap();

    assert_eq!(assets.len(), 3);
    assert!(assets.iter().all(|a| a.os == "windows"));
    let msi = assets
        .iter()
        .find(|a| a.name.ends_with("x86_64.msi"))
        .unwrap();
    assert_eq!((msi.arch.as_str(), msi.format.as_str()), ("x86_64", "msi"));
    let zip = assets
        .iter()
        .find(|a| a.name.ends_with("x86_64.zip"))
        .unwrap();
    assert_eq!((zip.arch.as_str(), zip.format.as_str()), ("x86_64", "zip"));
    assert!(assets.iter().any(|a| a.arch == "arm64"));
}

#[test]
fn collect_assets_skips_linux_packages_and_foreign_archives() {
    let dist = tempfile::tempdir().unwrap();
    for name in [
        "openlogi-v1.2.3-linux-amd64.deb",
        "openlogi-v1.2.3-linux-amd64.rpm",
        "not-an-artifact.zip",
        "SHA256SUMS",
    ] {
        fs_err::write(dist.path().join(name), b"artifact").unwrap();
        fs_err::write(dist.path().join(format!("{name}.minisig")), b"signature").unwrap();
    }

    let assets =
        collect_assets(dist.path(), "https://updates.example/releases/v1.2.3", true).unwrap();

    assert!(assets.is_empty());
}

#[test]
fn github_repo_manifest_uses_release_download_urls_and_gates_on_windows() {
    let dist = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let msi = "OpenLogi-v0.8.19010-windows-x86_64.msi";
    fs_err::write(dist.path().join(msi), b"msi").unwrap();
    fs_err::write(dist.path().join(format!("{msi}.minisig")), b"signature").unwrap();
    let output = stage.path().join("releases/v0.8.19010/feed/latest.json");
    let args = |dist: &Path| Args {
        dist: dist.to_path_buf(),
        stage: stage.path().to_path_buf(),
        tag: "v0.8.19010".to_owned(),
        base_url: "https://github.com/owner/OpenLogi/".to_owned(),
        include_windows: false,
        github_repo: Some("owner/OpenLogi".to_owned()),
    };

    run(&args(dist.path())).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(&fs_err::read_to_string(&output).unwrap()).unwrap();

    assert_eq!(
        manifest["assets"][0]["url"],
        format!("https://github.com/owner/OpenLogi/releases/download/v0.8.19010/{msi}")
    );
    assert_eq!(
        manifest["assets"][0]["signature_url"],
        format!("https://github.com/owner/OpenLogi/releases/download/v0.8.19010/{msi}.minisig")
    );
    assert_eq!(
        manifest["release_url"],
        "https://github.com/owner/OpenLogi/releases/tag/v0.8.19010"
    );
    assert_eq!(manifest["version"], "0.8.19010");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["channel"], "stable");
    assert_eq!(manifest["assets"][0]["format"], "msi");
    assert_eq!(manifest["assets"][0]["os"], "windows");
    assert_eq!(manifest["assets"][0]["arch"], "x86_64");
    assert_eq!(
        manifest["assets"][0]["sha256"],
        dist.path().join(msi).sha256().unwrap()
    );

    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        run(&args(empty.path())).unwrap_err().to_string(),
        "no Windows MSI assets found for manifest"
    );
}

#[test]
fn github_repo_gate_requires_an_msi_and_its_signature() {
    for (name, missing_signature) in [
        ("OpenLogi-v0.8.19010-windows-x86_64.zip", false),
        ("OpenLogi-v0.8.19010-macos-arm64.dmg", false),
        ("OpenLogi-v0.8.19010-windows-x86_64.msi", true),
    ] {
        let dist = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        fs_err::write(dist.path().join(name), b"artifact").unwrap();
        if !missing_signature {
            fs_err::write(dist.path().join(format!("{name}.minisig")), b"signature").unwrap();
        }
        let error = run(&Args {
            dist: dist.path().to_path_buf(),
            stage: stage.path().to_path_buf(),
            tag: "v0.8.19010".to_owned(),
            base_url: "https://github.com/owner/OpenLogi".to_owned(),
            include_windows: false,
            github_repo: Some("owner/OpenLogi".to_owned()),
        })
        .unwrap_err()
        .to_string();
        if missing_signature {
            assert!(error.contains("missing minisign signature"), "{error}");
        } else {
            assert_eq!(error, "no Windows MSI assets found for manifest");
        }
        assert!(
            !stage
                .path()
                .join("releases/v0.8.19010/feed/latest.json")
                .exists()
        );
    }
}

#[test]
fn upstream_release_records_keep_update_host_layout_and_channels() {
    for (tag, channel) in [("v0.8.19", "stable"), ("v0.8.20-beta.2", "beta")] {
        let dist = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let dmg = format!("OpenLogi-{tag}-macos-arm64.dmg");
        fs_err::write(dist.path().join(&dmg), b"dmg").unwrap();
        fs_err::write(dist.path().join(format!("{dmg}.minisig")), b"signature").unwrap();
        let args = Args {
            dist: dist.path().to_path_buf(),
            stage: stage.path().to_path_buf(),
            tag: tag.to_owned(),
            base_url: "https://updates.example/".to_owned(),
            include_windows: false,
            github_repo: None,
        };
        run(&args).unwrap();
        let output = stage
            .path()
            .join(format!("releases/{tag}/feed/latest.json"));
        let manifest: Manifest =
            serde_json::from_str(&fs_err::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(manifest.channel, channel);
        assert_eq!(manifest.version, tag.trim_start_matches('v'));
        assert_eq!(
            manifest.release_url,
            update_feed::release_page_url(&ReleaseTag::parse(tag).unwrap())
        );
        assert_eq!(
            manifest.assets[0].url,
            format!("https://updates.example/releases/{tag}/{dmg}")
        );

        fs_err::remove_file(dist.path().join(&dmg)).unwrap();
        assert_eq!(
            run(&args).unwrap_err().to_string(),
            "no architecture-specific DMG assets found for manifest"
        );
    }
}
