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
    pub fn read_range(&self, start: usize, len: usize) -> Vec<u8> {
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
