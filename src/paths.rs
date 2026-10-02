/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::path::PathBuf;

const COOKIE_MRP_PATH: &str = "mythroad/cookie.mrp";

#[cfg(target_os = "android")]
const USER_DATA_PATH_ENV: &str = "SKYMRP_USER_DATA_PATH";

#[cfg(target_os = "android")]
const USER_DATA_URI: &str = "content://org.skymrp.android.provider/root/root";

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

pub fn cookie_mrp_path() -> Result<PathBuf, String> {
    Ok(user_data_base_path()?.join(COOKIE_MRP_PATH))
}

pub fn ensure_mythroad_dir() -> Result<(), String> {
    let cookie_path = cookie_mrp_path()?;
    let directory = cookie_path
        .parent()
        .ok_or_else(|| format!("Invalid MRP path: {}", cookie_path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("Could not create {}: {e}", directory.display()))
}

pub fn url_for_opening_user_data_dir() -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        Ok(USER_DATA_URI.to_owned())
    }

    #[cfg(not(target_os = "android"))]
    {
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
}
