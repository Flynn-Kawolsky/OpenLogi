//! Build script for openlogi-desktop.
//!
//! Besides the existing update-manifest env hook, this embeds the Windows exe
//! resources (icon + VERSIONINFO).
//!
//! The upstream gpui-component theme files are deliberately *not* embedded:
//! this build's Settings → Appearance offers only the OpenLogi brand theme
//! (`themes/openlogi.json`, compiled in by `src/ui/theme.rs`) and the
//! framework's own built-in default light/dark pair.

#![expect(
    clippy::expect_used,
    reason = "a build script fails by panicking, so expect — whose message surfaces in the build log — is the idiomatic error path"
)]

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-env-changed=OPENLOGI_UPDATE_MANIFEST_URL");

    // `rust_i18n::i18n!` in `main.rs` reads `../openlogi-ui/locales` at macro
    // expansion time, and Cargo does not track a proc macro's file reads. Without
    // this, editing a catalog leaves the embedded translations stale until an
    // unrelated change happens to recompile this crate.
    println!("cargo:rerun-if-changed=../openlogi-ui/locales");

    embed_windows_resources();
}

/// Embed the Windows exe resources — the app icon and a VERSIONINFO block —
/// so Explorer, the taskbar, and Task Manager stop showing the generic blank
/// binary. The `.rc` is generated here rather than committed so the version
/// block tracks `CARGO_PKG_VERSION` (a literal would go stale the moment
/// release-plz bumps). No-op off Windows targets.
///
/// Kept in sync with the twin in `crates/openlogi-agent/build.rs` — only the
/// description/filename strings differ.
fn embed_windows_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // rc.exe treats `\` in string literals as escapes; forward slashes are
    // accepted by every resource compiler embed-resource can drive.
    let icon = manifest_dir
        .join("../../design/icon/openlogi.ico")
        .display()
        .to_string()
        .replace('\\', "/");
    println!("cargo:rerun-if-changed={icon}");

    let (major, minor, patch) = (
        env::var("CARGO_PKG_VERSION_MAJOR").expect("CARGO_PKG_VERSION_MAJOR"),
        env::var("CARGO_PKG_VERSION_MINOR").expect("CARGO_PKG_VERSION_MINOR"),
        env::var("CARGO_PKG_VERSION_PATCH").expect("CARGO_PKG_VERSION_PATCH"),
    );
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "AprilNEA"
            VALUE "FileDescription", "OpenLogi"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "openlogi-desktop"
            VALUE "OriginalFilename", "OpenLogi.exe"
            VALUE "ProductName", "OpenLogi"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let rc_path = out.join("openlogi-desktop.rc");
    fs::write(&rc_path, rc).expect("write generated .rc into OUT_DIR");
    // manifest_optional: a missing resource compiler downgrades to a cargo
    // warning (icon-less but working exe) instead of failing dev builds on
    // machines without the Windows SDK; release builds run on CI runners that
    // always carry rc.exe.
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile Windows resources");
}
