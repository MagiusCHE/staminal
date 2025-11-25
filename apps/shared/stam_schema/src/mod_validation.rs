use std::collections::HashMap;

use semver::Version;

use crate::{ModManifest, parse_version_requirement};

/// Validate if a version is within the specified range.
/// min_version and max_version should be in format "major.minor.patch".
/// Returns Ok(()) if version is in range, Err with message otherwise.
pub fn validate_version_range(
    context: &str,
    installed_version: &str,
    min_version: &str,
    max_version: &str,
) -> Result<(), String> {
    // Parse installed version
    let installed = Version::parse(installed_version).map_err(|e| {
        format!(
            "{}: Invalid installed version '{}': {}",
            context, installed_version, e
        )
    })?;

    // Parse min and max versions
    let min = Version::parse(min_version)
        .map_err(|e| format!("{}: Invalid min_version '{}': {}", context, min_version, e))?;

    let max = Version::parse(max_version)
        .map_err(|e| format!("{}: Invalid max_version '{}': {}", context, max_version, e))?;

    // Check if installed version is within range (inclusive on both ends)
    if installed < min {
        return Err(format!(
            "{}: version {} is below minimum required version {}",
            context, installed_version, min_version
        ));
    }

    if installed > max {
        return Err(format!(
            "{}: version {} is above maximum supported version {}",
            context, installed_version, max_version
        ));
    }

    Ok(())
}

/// Validate mod dependencies.
/// Checks:
/// - "@client" requirement against client_version
/// - "@game" requirement against game_version
/// - "@server" requirement against server_version
/// - Other mod requirements against loaded manifests
pub fn validate_mod_dependencies(
    mod_id: &str,
    manifest: &ModManifest,
    all_manifests: &HashMap<String, ModManifest>,
    client_version: &str,
    game_version: &str,
    server_version: &str,
    skip_client_requirement: bool,
) -> Result<(), String> {
    for (dep_id, version_req) in &manifest.requires {
        let (min_ver, max_ver) = parse_version_requirement(version_req);

        if dep_id == "@client" {
            if skip_client_requirement {
                continue;
            }
            // Validate against client version
            validate_version_range(
                &format!("Mod '{}' requires client", mod_id),
                client_version,
                &min_ver,
                &max_ver,
            )?;
        } else if dep_id == "@game" {
            // Validate against active game version from server
            validate_version_range(
                &format!("Mod '{}' requires game", mod_id),
                game_version,
                &min_ver,
                &max_ver,
            )?;
        } else if dep_id == "@server" {
            // Validate against server version received during handshake
            validate_version_range(
                &format!("Mod '{}' requires server", mod_id),
                server_version,
                &min_ver,
                &max_ver,
            )?;
        } else {
            // Validate against another mod's version
            if let Some(dep_manifest) = all_manifests.get(dep_id) {
                validate_version_range(
                    &format!("Mod '{}' requires '{}'", mod_id, dep_id),
                    &dep_manifest.version,
                    &min_ver,
                    &max_ver,
                )?;
            } else {
                return Err(format!(
                    "Mod '{}' requires mod '{}' which is not available",
                    mod_id, dep_id
                ));
            }
        }
    }

    Ok(())
}
