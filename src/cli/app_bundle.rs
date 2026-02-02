use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;

/// Create a macOS .app bundle at `~/.threader/Threader.app` that registers
/// the `threader://` URL scheme. When macOS opens a `threader://` URL, it
/// launches this app which delegates to `threader handle-url <url>`.
pub fn create_app_bundle() -> Result<()> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let threader_dir = home.join(".threader");
    let app_path = threader_dir.join("Threader.app");

    // Remove existing bundle for idempotency
    if app_path.exists() {
        fs::remove_dir_all(&app_path)
            .with_context(|| format!("Failed to remove existing app bundle: {}", app_path.display()))?;
    }

    fs::create_dir_all(&threader_dir)?;

    // Resolve threader binary path
    let threader_bin = home.join(".local").join("bin").join("threader");
    let threader_cmd = if threader_bin.exists() {
        threader_bin
            .to_str()
            .context("Non-UTF8 path to threader binary")?
            .to_string()
    } else {
        "threader".to_string()
    };

    // Write AppleScript source that handles URL open events
    let applescript = format!(
        r#"on open location this_URL
    do shell script "{threader_cmd} handle-url " & quoted form of this_URL & " &> /dev/null &"
end open location"#
    );

    let script_path = threader_dir.join("url_handler.applescript");
    fs::write(&script_path, &applescript)?;

    // Compile into .app bundle
    let output = Command::new("osacompile")
        .args(["-o"])
        .arg(&app_path)
        .arg(&script_path)
        .output()
        .context("Failed to run osacompile")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("osacompile failed: {}", stderr.trim());
    }

    // Clean up source file
    let _ = fs::remove_file(&script_path);

    // Patch Info.plist to register URL scheme
    let plist_path = app_path.join("Contents").join("Info.plist");
    patch_info_plist(&plist_path)?;

    // Register with Launch Services
    let output = Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        .args(["-R", "-f"])
        .arg(&app_path)
        .output()
        .context("Failed to run lsregister")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("lsregister failed: {}", stderr.trim());
    }

    info!("Created URL handler app at {}", app_path.display());
    Ok(())
}

/// Patch the Info.plist to add URL scheme registration and hide from dock.
fn patch_info_plist(plist_path: &std::path::Path) -> Result<()> {
    // Read existing plist (osacompile generates an XML plist)
    let content = fs::read_to_string(plist_path)
        .with_context(|| format!("Failed to read {}", plist_path.display()))?;

    // Insert our keys before the closing </dict></plist>
    let additions = r#"
	<key>CFBundleIdentifier</key>
	<string>sh.threader.URLHandler</string>
	<key>CFBundleURLTypes</key>
	<array>
		<dict>
			<key>CFBundleURLName</key>
			<string>Threader URL</string>
			<key>CFBundleURLSchemes</key>
			<array>
				<string>threader</string>
			</array>
		</dict>
	</array>
	<key>LSUIElement</key>
	<true/>
"#;

    let patched = content.replace("</dict>\n</plist>", &format!("{additions}</dict>\n</plist>"));

    fs::write(plist_path, &patched)
        .with_context(|| format!("Failed to write patched {}", plist_path.display()))?;

    Ok(())
}
