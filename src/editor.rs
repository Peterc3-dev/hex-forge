use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

/// Memory-mapped file editor.
/// Reads via mmap, tracks edits in an overlay HashMap.
/// Only modified bytes are held in memory — the rest is read from the mmap.
pub struct Editor {
    path: PathBuf,
    mmap: Option<Mmap>,
    file_size: usize,
    /// Overlay: offset -> modified byte
    edits: HashMap<usize, u8>,
    pub readonly: bool,
}

impl Editor {
    pub fn open(path: &Path, readonly: bool) -> io::Result<Self> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        if file_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot open empty file",
            ));
        }

        let mmap = unsafe { Mmap::map(&file)? };

        Ok(Self {
            path: path.to_path_buf(),
            mmap: Some(mmap),
            file_size,
            edits: HashMap::new(),
            readonly,
        })
    }

    pub fn file_size(&self) -> usize {
        self.file_size
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read a single byte (overlay takes priority).
    pub fn read_byte(&self, offset: usize) -> Option<u8> {
        if offset >= self.file_size {
            return None;
        }
        if let Some(&b) = self.edits.get(&offset) {
            Some(b)
        } else {
            self.mmap.as_ref().map(|m| m[offset])
        }
    }

    /// Read a range of bytes into a Vec.
    ///
    /// The range is clamped to the file. If `start` is past EOF, an empty
    /// Vec is returned (callers treat a short read as "field unavailable").
    pub fn read_range(&self, start: usize, len: usize) -> Vec<u8> {
        if start >= self.file_size {
            return Vec::new();
        }
        let end = (start + len).min(self.file_size);
        let mut buf = Vec::with_capacity(end - start);
        for i in start..end {
            buf.push(self.read_byte(i).unwrap_or(0));
        }
        buf
    }

    /// Check if a byte at the given offset has been modified.
    pub fn is_modified(&self, offset: usize) -> bool {
        self.edits.contains_key(&offset)
    }

    /// Write a byte (stores in overlay).
    pub fn write_byte(&mut self, offset: usize, value: u8) -> bool {
        if self.readonly || offset >= self.file_size {
            return false;
        }
        self.edits.insert(offset, value);
        true
    }

    /// Whether there are unsaved changes.
    #[allow(dead_code)]
    pub fn has_edits(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Flush edits to disk.
    pub fn save(&mut self) -> io::Result<()> {
        if self.readonly {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "File is read-only",
            ));
        }
        if self.edits.is_empty() {
            return Ok(());
        }

        // 1. Collect edits from the overlay
        let edits: Vec<(usize, u8)> = self.edits.drain().collect();

        // 2. Drop the mmap so the file is no longer mapped
        self.mmap = None;

        // 3. Open file for writing, apply edits, sync
        let write_result = (|| -> io::Result<()> {
            use std::os::unix::fs::FileExt;
            let file = OpenOptions::new().write(true).open(&self.path)?;
            for &(offset, byte) in &edits {
                file.write_at(&[byte], offset as u64)?;
            }
            file.sync_all()?;
            Ok(())
        })();

        // 4. Re-mmap the file (regardless of write success/failure)
        let remap_result = (|| -> io::Result<()> {
            let file = File::open(&self.path)?;
            self.mmap = Some(unsafe { Mmap::map(&file)? });
            Ok(())
        })();

        // 5. If write failed, restore edits into the overlay so they aren't lost
        if let Err(e) = write_result {
            for (offset, byte) in edits {
                self.edits.insert(offset, byte);
            }
            return Err(e);
        }

        // 6. If re-mmap failed, restore edits (file state is uncertain)
        if let Err(e) = remap_result {
            for (offset, byte) in edits {
                self.edits.insert(offset, byte);
            }
            return Err(e);
        }

        // Success — overlay is already drained, mmap is fresh
        Ok(())
    }

    /// Search for a byte pattern starting from a given offset.
    /// Returns the offset of the first match, or None.
    pub fn search(&self, pattern: &[u8], start: usize) -> Option<usize> {
        if pattern.is_empty() || start >= self.file_size {
            return None;
        }
        let end = self.file_size.saturating_sub(pattern.len());
        for i in start..=end {
            let mut found = true;
            for (j, &pb) in pattern.iter().enumerate() {
                if self.read_byte(i + j) != Some(pb) {
                    found = false;
                    break;
                }
            }
            if found {
                return Some(i);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp file with the given contents and open it as an Editor.
    fn editor_with(bytes: &[u8], readonly: bool) -> (Editor, tempdir::TempPath) {
        let path = tempdir::TempPath::new(bytes);
        let editor = Editor::open(path.as_path(), readonly).unwrap();
        (editor, path)
    }

    /// Minimal self-contained temp-file helper (no external dev-dependency).
    mod tempdir {
        use std::fs;
        use std::io::Write;
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        pub struct TempPath(PathBuf);

        impl TempPath {
            pub fn new(bytes: &[u8]) -> Self {
                let n = COUNTER.fetch_add(1, Ordering::Relaxed);
                let mut path = std::env::temp_dir();
                path.push(format!("hex-forge-test-{}-{}.bin", std::process::id(), n));
                let mut f = fs::File::create(&path).unwrap();
                f.write_all(bytes).unwrap();
                f.sync_all().unwrap();
                TempPath(path)
            }

            pub fn as_path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
    }

    #[test]
    fn read_byte_and_range_from_mmap() {
        let (editor, _p) = editor_with(&[0x10, 0x20, 0x30, 0x40], false);
        assert_eq!(editor.file_size(), 4);
        assert_eq!(editor.read_byte(0), Some(0x10));
        assert_eq!(editor.read_byte(3), Some(0x40));
        assert_eq!(editor.read_byte(4), None);
        assert_eq!(editor.read_range(1, 2), vec![0x20, 0x30]);
        // read_range is clamped to file size
        assert_eq!(editor.read_range(2, 10), vec![0x30, 0x40]);
        // start at or past EOF yields an empty Vec (no underflow panic)
        assert_eq!(editor.read_range(4, 4), Vec::<u8>::new());
        assert_eq!(editor.read_range(100, 8), Vec::<u8>::new());
    }

    #[test]
    fn write_byte_overlays_without_touching_disk() {
        let (mut editor, _p) = editor_with(&[0xAA, 0xBB], false);
        assert!(!editor.is_modified(0));
        assert!(editor.write_byte(0, 0xFF));
        assert_eq!(editor.read_byte(0), Some(0xFF));
        assert!(editor.is_modified(0));
        assert!(editor.has_edits());
        // Out-of-range write is rejected
        assert!(!editor.write_byte(99, 0x00));
    }

    #[test]
    fn readonly_rejects_writes() {
        let (mut editor, _p) = editor_with(&[0x01, 0x02], true);
        assert!(!editor.write_byte(0, 0xFF));
        assert_eq!(editor.read_byte(0), Some(0x01));
        assert!(!editor.has_edits());
    }

    #[test]
    fn empty_file_is_rejected() {
        let p = tempdir::TempPath::new(&[]);
        match Editor::open(p.as_path(), false) {
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::InvalidInput),
            Ok(_) => panic!("expected empty file to be rejected"),
        }
    }

    #[test]
    fn search_finds_pattern_and_respects_start() {
        let (editor, _p) = editor_with(&[0x00, 0xDE, 0xAD, 0xBE, 0xEF, 0xDE, 0xAD], false);
        assert_eq!(editor.search(&[0xDE, 0xAD], 0), Some(1));
        // starting past the first match finds the second occurrence
        assert_eq!(editor.search(&[0xDE, 0xAD], 2), Some(5));
        assert_eq!(editor.search(&[0x12, 0x34], 0), None);
        // empty pattern / out-of-range start return None
        assert_eq!(editor.search(&[], 0), None);
        assert_eq!(editor.search(&[0x00], 99), None);
    }

    #[test]
    fn search_reflects_overlay_edits() {
        let (mut editor, _p) = editor_with(&[0x00, 0x00, 0x00], false);
        assert_eq!(editor.search(&[0xCA, 0xFE], 0), None);
        editor.write_byte(1, 0xCA);
        editor.write_byte(2, 0xFE);
        assert_eq!(editor.search(&[0xCA, 0xFE], 0), Some(1));
    }

    #[test]
    fn save_persists_edits_to_disk() {
        let p = tempdir::TempPath::new(&[0x01, 0x02, 0x03]);
        {
            let mut editor = Editor::open(p.as_path(), false).unwrap();
            editor.write_byte(1, 0xFF);
            editor.save().unwrap();
            assert!(!editor.has_edits());
        }
        // Re-read from a fresh handle to confirm the byte hit disk.
        let reread = std::fs::read(p.as_path()).unwrap();
        assert_eq!(reread, vec![0x01, 0xFF, 0x03]);
    }
}
