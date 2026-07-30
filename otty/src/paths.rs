//! Platform-aware filesystem locations shared across widgets.

use std::path::PathBuf;

/// Directory holding otty's configuration files (settings, shell
/// integration scripts).
///
/// - unix: `$HOME/.config/otty`
/// - windows: `%APPDATA%\otty`
/// - fallback: `<temp dir>/otty`
pub(crate) fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").ok();
    let appdata = std::env::var("APPDATA").ok();
    let temp = std::env::temp_dir();

    config_dir_from(
        home.as_deref(),
        appdata.as_deref(),
        temp.to_str().unwrap_or("/tmp"),
    )
}

/// Current user's home directory, if the platform exposes one.
///
/// unix reads `$HOME`; windows prefers `%USERPROFILE%` with a `$HOME`
/// fallback for MSYS2/Git Bash environments.
pub(crate) fn home_dir() -> Option<String> {
    let home = std::env::var("HOME").ok();
    let userprofile = std::env::var("USERPROFILE").ok();

    home_dir_from(home.as_deref(), userprofile.as_deref())
}

fn config_dir_from(
    home: Option<&str>,
    appdata: Option<&str>,
    temp: &str,
) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = home;
        if let Some(appdata) = appdata {
            return PathBuf::from(appdata).join("otty");
        }

        PathBuf::from(temp).join("otty")
    }

    #[cfg(not(windows))]
    {
        let _ = appdata;
        if let Some(home) = home {
            return PathBuf::from(home).join(".config").join("otty");
        }

        PathBuf::from(temp).join("otty")
    }
}

fn home_dir_from(
    home: Option<&str>,
    userprofile: Option<&str>,
) -> Option<String> {
    #[cfg(windows)]
    {
        return userprofile.or(home).map(ToString::to_string);
    }

    #[cfg(not(windows))]
    {
        let _ = userprofile;
        home.map(ToString::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::{config_dir_from, home_dir_from};

    #[cfg(not(windows))]
    #[test]
    fn given_home_when_config_dir_then_dot_config_otty() {
        let dir = config_dir_from(Some("/home/u"), None, "/tmp");
        assert_eq!(dir, std::path::PathBuf::from("/home/u/.config/otty"));
    }

    #[cfg(windows)]
    #[test]
    fn given_appdata_when_config_dir_then_appdata_otty() {
        // Windows branch: APPDATA wins over HOME-shaped input.
        let dir = config_dir_from(
            None,
            Some("C:\\Users\\u\\AppData\\Roaming"),
            "/tmp",
        );
        assert_eq!(
            dir,
            std::path::PathBuf::from("C:\\Users\\u\\AppData\\Roaming")
                .join("otty")
        );
    }

    #[test]
    fn given_nothing_when_config_dir_then_temp_otty() {
        let dir = config_dir_from(None, None, "/fallback");
        assert_eq!(dir, std::path::PathBuf::from("/fallback").join("otty"));
    }

    #[cfg(not(windows))]
    #[test]
    fn given_home_when_home_dir_then_some() {
        assert_eq!(
            home_dir_from(Some("/home/u"), None),
            Some(String::from("/home/u"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn given_userprofile_when_home_dir_then_some() {
        assert_eq!(
            home_dir_from(None, Some("C:\\Users\\u")),
            Some(String::from("C:\\Users\\u"))
        );
    }

    #[test]
    fn given_nothing_when_home_dir_then_none() {
        assert_eq!(home_dir_from(None, None), None);
    }
}
