use std::path::Path;
use std::process::Command;

/// Opens a file or folder in the editor named in Settings → Tools.
///
/// # Arguments
/// * `editor` - the editor command, such as `code` or `"C:\Program Files\Sublime Text\subl.exe"`
/// * `path` - what to open
///
/// # Returns
/// `false` when no editor is set or it could not be started, so the caller can fall back to the
/// system's default program.
pub(crate) fn open_in_editor(editor: &str, path: &Path) -> bool {
    let editor = editor.trim();
    if editor.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .arg("/C")
            .raw_arg(format!("{editor} \"{}\"", path.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path).spawn().is_ok()
    }
}

pub(crate) fn open_terminal(dir: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        if Command::new("wt.exe").arg("-d").arg(dir).spawn().is_ok() {
            return;
        }
        let _ = Command::new("powershell.exe")
            .current_dir(dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").args(["-a", "Terminal"]).arg(dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for term in ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"] {
            if Command::new(term).current_dir(dir).spawn().is_ok() {
                break;
            }
        }
    }
}
