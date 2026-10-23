/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::path::PathBuf;

const MYTHROAD_DIRECTORY: &str = "mythroad";
const DEFAULT_MRP_FILENAMES: [&str; 2] = ["dsm_gm.mrp", "cookie.mrp"];

#[cfg(target_os = "android")]
const USER_DATA_PATH_ENV: &str = "SKYMRP_USER_DATA_PATH";

fn user_data_base_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    {
        std::env::var_os(USER_DATA_PATH_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{USER_DATA_PATH_ENV} is not set"))
    }

    #[cfg(not(target_os = "android"))]
    {
        std::env::current_dir().map_err(|e| format!("Could not get current directory: {e}"))
    }
}

fn mythroad_path() -> Result<PathBuf, String> {
    Ok(user_data_base_path()?.join(MYTHROAD_DIRECTORY))
}

pub fn default_mrp_paths() -> Result<[PathBuf; 2], String> {
    let directory = mythroad_path()?;
    Ok(DEFAULT_MRP_FILENAMES.map(|filename| directory.join(filename)))
}

pub fn ensure_mythroad_dir() -> Result<(), String> {
    let directory = mythroad_path()?;
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("Could not create {}: {e}", directory.display()))
}

#[cfg(not(target_os = "android"))]
pub fn url_for_opening_user_data_dir() -> Result<String, String> {
    let path = user_data_base_path()?
        .canonicalize()
        .map_err(|e| format!("Could not resolve user data directory: {e}"))?;
    let path = path
        .to_str()
        .ok_or_else(|| "User data directory path is not UTF-8".to_owned())?;
    let path = if cfg!(target_os = "windows") {
        path.strip_prefix(r"\\?\").unwrap_or(path)
    } else {
        path
    };
    Ok(format!("file://{path}"))
}
