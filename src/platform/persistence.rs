use std::io::{self, Write};
use std::path::Path;

/// Atomic replacement on all supported platforms. Failure preserves the old file.
pub fn write_atomic(path: &Path, content: impl AsRef<[u8]>) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(content.as_ref())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|failure| failure.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace_unicode_file_without_losing_previous_data_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("曲 é.conf");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert!(write_atomic(dir.path(), "bad").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }
}
