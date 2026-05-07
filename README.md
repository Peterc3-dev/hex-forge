# hex-forge

Terminal hex editor with ELF, GGUF, PE, and PNG header parsing.

## Features

- Hex and ASCII edit modes with live byte editing
- Info view (F1) — auto-parses ELF, GGUF, PE/COFF, and PNG headers
- ELF: class, endianness, machine, entry point, section/program headers
- GGUF: version, tensor count, metadata key-value pairs
- PE: DOS header, COFF fields, optional header, sections
- Byte search (hex pattern or ASCII string) with find-next
- Goto offset (hex with 0x prefix or decimal)
- Visual selection and copy
- Read-only mode (`-r`)
- Memory-mapped file I/O for large binaries

## Install

```
cargo build --release
```

Binary lands at `target/release/hex-forge`.

## Usage

Open a file:

```
hex-forge firmware.bin
```

Open read-only:

```
hex-forge -r /usr/bin/ls
```

## Keybindings

| Key             | Action                          |
|-----------------|---------------------------------|
| Arrow keys      | Move cursor                     |
| `PgUp` / `PgDn`| Scroll page                     |
| `Home` / `End`  | Start / end of line             |
| `Ctrl-Home/End` | Start / end of file             |
| `Tab`           | Toggle hex / ASCII edit mode    |
| `F1`            | Toggle hex view / header info   |
| `/` or `Ctrl-F` | Search (hex: `FF 00`, ascii: `text`) |
| `n`             | Find next match                 |
| `Ctrl-G`        | Goto offset                     |
| `v`             | Start/clear selection           |
| `y`             | Copy selection                  |
| `Ctrl-S`        | Save                            |
| `Ctrl-Q`        | Quit (confirms if unsaved)      |
| `Esc`           | Clear selection / status        |

---

Built with Rust + ratatui + memmap2.
