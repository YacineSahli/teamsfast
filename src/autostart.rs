//! Login autostart: manage the XDG autostart desktop entry for the
//! CURRENT executable (dev builds point at the debug binary — flipping
//! the toggle is honest about what will start).

/// `~/.config/autostart/teamsfast.desktop`
fn desktop_path() -> std::path::PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                std::path::PathBuf::from(h).join(".config")
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    config.join("autostart").join("teamsfast.desktop")
}

/// Whether the autostart entry currently exists and is not disabled.
pub fn enabled() -> bool {
    let Ok(body) = std::fs::read_to_string(desktop_path()) else {
        return false;
    };
    !body.contains("X-GNOME-Autostart-enabled=false")
}

/// Write or remove the entry. `on` writes one pointing at this binary.
pub fn set(on: bool) -> std::result::Result<(), String> {
    let path = desktop_path();
    if !on {
        // Absent is fine; a leftover directory structure is not ours to
        // clean beyond the file itself.
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("remove {}: {e}", path.display())),
        }
    } else {
        let Some(exe) = std::env::current_exe().ok() else {
            return Err("cannot resolve the running executable".into());
        };
        let body = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=TeamsFast\n\
             Comment=Microsoft Teams client\n\
             Exec={}\n\
             TryExec={}\n\
             Terminal=false\n\
             Categories=Network;InstantMessaging;\n\
             X-GNOME-Autostart-enabled=true\n",
            exe.display(),
            exe.display(),
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, body)
            .map_err(|e| format!("write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The XDG dirs are env-driven; the test redirects to a temp dir so it
    // never touches the real autostart.
    #[test]
    fn write_and_remove_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tf-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert!(!enabled());
        set(true).expect("write");
        assert!(enabled());
        assert!(desktop_path().exists());
        let body = std::fs::read_to_string(desktop_path()).unwrap();
        assert!(body.contains("[Desktop Entry]"), "entry shape");
        assert!(body.contains("Exec="), "points at an executable");
        set(false).expect("remove");
        assert!(!enabled());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
