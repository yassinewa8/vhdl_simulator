//! Source files open in editor tabs: their text, whether it's saved, and
//! whether the file changed on disk meanwhile. No GUI code, so it can be
//! tested; `editor_ui.rs` draws it.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often to look at open files' modification times.
const DISK_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// One file open in a tab.
#[derive(Debug)]
pub struct OpenFile {
    /// Full path on disk.
    pub path: PathBuf,
    /// The text as being edited, always with `\n` line ends.
    pub text: String,
    /// The text as last loaded or saved, to tell whether there are changes.
    saved_text: String,
    /// The file used Windows (`\r\n`) line ends; saving keeps them.
    crlf: bool,
    /// Why the file can't be edited, if it can't (e.g. not UTF-8 text:
    /// saving it back could damage characters we couldn't read).
    pub read_only: Option<String>,
    /// Modification time when last loaded or saved.
    stamp: Option<SystemTime>,
    /// The file changed on disk while there were unsaved edits here. The
    /// user decides which version wins.
    pub changed_on_disk: bool,
    /// Why the last save failed, shown above the text until a save works.
    pub save_error: Option<String>,
}

impl OpenFile {
    pub fn load(path: &Path) -> std::io::Result<OpenFile> {
        let mut file = OpenFile {
            path: path.to_path_buf(),
            text: String::new(),
            saved_text: String::new(),
            crlf: false,
            read_only: None,
            stamp: None,
            changed_on_disk: false,
            save_error: None,
        };
        file.reload()?;
        Ok(file)
    }

    /// The file name, for the tab.
    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// True if there are edits not saved yet.
    pub fn is_modified(&self) -> bool {
        self.text != self.saved_text
    }

    /// Reads the file again, dropping any edits.
    pub fn reload(&mut self) -> std::io::Result<()> {
        let bytes = std::fs::read(&self.path)?;
        let (text, read_only) = match String::from_utf8(bytes) {
            Ok(text) => (text, None),
            Err(error) => (
                String::from_utf8_lossy(error.as_bytes()).into_owned(),
                Some("Read-only: the file isn't UTF-8 text, so saving could damage some characters.".to_string()),
            ),
        };
        self.crlf = text.contains("\r\n");
        self.text = text.replace("\r\n", "\n");
        self.saved_text = self.text.clone();
        self.read_only = read_only;
        self.stamp = modified_time(&self.path);
        self.changed_on_disk = false;
        Ok(())
    }

    /// Writes the edits to disk, with the file's own line ends.
    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(reason) = &self.read_only {
            return Err(std::io::Error::other(reason.clone()));
        }
        let text = if self.crlf { self.text.replace('\n', "\r\n") } else { self.text.clone() };
        std::fs::write(&self.path, text)?;
        self.saved_text = self.text.clone();
        self.stamp = modified_time(&self.path);
        self.changed_on_disk = false;
        self.save_error = None;
        Ok(())
    }

    /// The user chose to keep their edits over the changed file on disk:
    /// stop warning, and saving will overwrite it.
    pub fn keep_mine(&mut self) {
        self.stamp = modified_time(&self.path);
        self.changed_on_disk = false;
    }

    /// Looks at the file on disk. If it changed and there are no edits
    /// here, it's simply reloaded; with edits, `changed_on_disk` is set.
    fn check_disk(&mut self) {
        let stamp = modified_time(&self.path);
        if stamp.is_none() || stamp == self.stamp || self.changed_on_disk {
            return;
        }
        if self.is_modified() {
            self.changed_on_disk = true;
        } else {
            // If it can't be read just now (e.g. half written), the next
            // check tries again: the stamp is only updated on success.
            let _ = self.reload();
        }
    }
}

/// All open files, in tab order.
#[derive(Debug, Default)]
pub struct Editors {
    pub files: Vec<OpenFile>,
    /// Index of the tab being shown (when a file tab is shown at all).
    pub active: usize,
    last_check: Option<Instant>,
}

impl Editors {
    /// Opens `path` in a tab, or switches to it if it's open already.
    pub fn open(&mut self, path: &Path) -> std::io::Result<()> {
        if let Some(index) = self.files.iter().position(|file| same_path(&file.path, path)) {
            self.active = index;
            return Ok(());
        }
        self.files.push(OpenFile::load(path)?);
        self.active = self.files.len() - 1;
        Ok(())
    }

    #[cfg(test)]
    pub fn active_file(&mut self) -> Option<&mut OpenFile> {
        self.files.get_mut(self.active)
    }

    /// Closes a tab, dropping any unsaved edits (ask first!).
    pub fn close(&mut self, index: usize) {
        if index >= self.files.len() {
            return;
        }
        self.files.remove(index);
        // Keep showing the same tab, or the one that took its place.
        if self.active > index || self.active >= self.files.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Files with unsaved edits.
    pub fn modified_count(&self) -> usize {
        self.files.iter().filter(|file| file.is_modified()).count()
    }

    /// Notices files changed on disk (e.g. edited in another editor, or by
    /// a `git checkout`). Call once per frame; it only looks twice a second.
    pub fn check_disk(&mut self, now: Instant) {
        if self.last_check.is_some_and(|last| now.duration_since(last) < DISK_CHECK_INTERVAL) {
            return;
        }
        self.last_check = Some(now);
        for file in &mut self.files {
            file.check_disk();
        }
    }
}

/// Where `needle` next appears in `text` after character `from`, ignoring
/// case, as a range of character positions (what the text editor works
/// in). Wraps round to the top. `None` if it isn't there at all.
pub fn find_next(text: &str, needle: &str, from: usize) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    // ASCII lower-casing keeps byte positions the same, so positions found
    // in the lower-case copy are valid in the original.
    let haystack = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let from_byte = text.char_indices().nth(from).map_or(text.len(), |(byte, _)| byte);
    let found = haystack[from_byte..]
        .find(&needle)
        .map(|at| from_byte + at)
        .or_else(|| haystack.find(&needle))?;
    let start = text[..found].chars().count();
    Some(start..start + needle.chars().count())
}

/// True if two paths name the same file, ignoring `/` vs `\` and, on
/// Windows, letter case.
fn same_path(a: &Path, b: &Path) -> bool {
    let normalise = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        if cfg!(windows) { text.to_lowercase() } else { text }
    };
    normalise(a) == normalise(b)
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("nvc-studio-tests").join(test);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Makes the file look changed on disk, whatever the clock resolution.
    fn touch_later(path: &Path) {
        let later = SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(later).unwrap();
    }

    #[test]
    fn edits_are_saved_with_the_files_own_line_ends() {
        let dir = scratch("editor_line_ends");
        let path = dir.join("a.vhd");
        std::fs::write(&path, "entity a is\r\nend;\r\n").unwrap();

        let mut file = OpenFile::load(&path).unwrap();
        assert_eq!(file.text, "entity a is\nend;\n", "edited with \\n only");
        assert!(!file.is_modified());
        file.text.insert_str(0, "-- new\n");
        assert!(file.is_modified());
        file.save().unwrap();
        assert!(!file.is_modified());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "-- new\r\nentity a is\r\nend;\r\n");
    }

    #[test]
    fn find_goes_forward_ignoring_case_and_wraps() {
        let text = "signal Clk : std_logic; -- café clk
clk <= not CLK;";
        assert_eq!(find_next(text, "clk", 0), Some(7..10));
        assert_eq!(find_next(text, "clk", 10), Some(32..35), "counts characters, not bytes (é)");
        assert_eq!(find_next(text, "clk", 35), Some(36..39));
        assert_eq!(find_next(text, "clk", 40), Some(47..50));
        assert_eq!(find_next(text, "CLK", 50), Some(7..10), "wraps to the top");
        assert_eq!(find_next(text, "nothing", 0), None);
        assert_eq!(find_next(text, "", 0), None);
    }

    #[test]
    fn non_utf8_files_are_read_only() {
        let dir = scratch("editor_not_utf8");
        let path = dir.join("latin1.vhd");
        std::fs::write(&path, b"-- caf\xe9\nentity a is end;\n").unwrap();
        let mut file = OpenFile::load(&path).unwrap();
        assert!(file.read_only.is_some());
        assert!(file.text.contains("entity a"));
        file.text.push('x');
        assert!(file.save().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"-- caf\xe9\nentity a is end;\n", "untouched");
    }

    #[test]
    fn changes_on_disk_reload_unless_there_are_edits() {
        let dir = scratch("editor_disk_changes");
        let path = dir.join("a.vhd");
        std::fs::write(&path, "one\n").unwrap();
        let mut editors = Editors::default();
        editors.open(&path).unwrap();
        let start = Instant::now();

        // No edits here: the new text simply appears.
        std::fs::write(&path, "two\n").unwrap();
        touch_later(&path);
        editors.check_disk(start);
        assert_eq!(editors.files[0].text, "two\n");
        assert!(!editors.files[0].changed_on_disk);

        // With edits: nothing is lost; the user is warned instead.
        editors.files[0].text.push_str("mine\n");
        std::fs::write(&path, "three\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(20))
            .unwrap();
        editors.check_disk(start + Duration::from_millis(100));
        assert!(!editors.files[0].changed_on_disk, "not checked again so soon");
        editors.check_disk(start + Duration::from_secs(1));
        assert!(editors.files[0].changed_on_disk);
        assert_eq!(editors.files[0].text, "two\nmine\n");

        // Keeping mine stops the warning; saving overwrites the disk.
        editors.files[0].keep_mine();
        assert!(!editors.files[0].changed_on_disk);
        editors.files[0].save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\nmine\n");
    }

    #[test]
    fn opening_twice_switches_to_the_tab_and_closing_keeps_a_sensible_one() {
        let dir = scratch("editor_tabs");
        let paths: Vec<PathBuf> = ["a.vhd", "b.vhd", "c.vhd"].iter().map(|name| dir.join(name)).collect();
        let mut editors = Editors::default();
        for path in &paths {
            std::fs::write(path, "").unwrap();
            editors.open(path).unwrap();
        }
        assert_eq!(editors.active, 2);
        let written_differently = PathBuf::from(paths[0].to_string_lossy().replace('\\', "/"));
        editors.open(&written_differently).unwrap();
        assert_eq!((editors.files.len(), editors.active), (3, 0));
        assert!(editors.open(&dir.join("missing.vhd")).is_err());

        editors.active = 2;
        editors.close(0); // a tab before the shown one
        assert_eq!(editors.active_file().unwrap().title(), "c.vhd");
        editors.close(1); // the shown one, which was last
        assert_eq!(editors.active_file().unwrap().title(), "b.vhd");
        editors.close(0);
        assert!(editors.active_file().is_none());
    }
}
