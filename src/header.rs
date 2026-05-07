use crate::editor::Editor;

/// Parsed header information for display in the info view.
pub struct HeaderInfo {
    pub format_name: String,
    pub fields: Vec<(String, String)>,
}

pub fn parse_header(editor: &Editor) -> HeaderInfo {
    let magic = editor.read_range(0, 8);

    // ELF: 0x7f 'E' 'L' 'F'
    if magic.len() >= 4 && magic[0] == 0x7f && magic[1] == b'E' && magic[2] == b'L' && magic[3] == b'F' {
        return parse_elf(editor);
    }

    // GGUF: 'G' 'G' 'U' 'F'
    if magic.len() >= 4 && magic[0] == b'G' && magic[1] == b'G' && magic[2] == b'U' && magic[3] == b'F' {
        return parse_gguf(editor);
    }

    // PE/COFF: 'M' 'Z'
    if magic.len() >= 2 && magic[0] == b'M' && magic[1] == b'Z' {
        return parse_pe(editor);
    }

    // PNG
    if magic.len() >= 8
        && magic[0] == 0x89
        && magic[1] == b'P'
        && magic[2] == b'N'
        && magic[3] == b'G'
    {
        return parse_png(editor);
    }

    parse_generic(editor)
}

fn read_u16_le(editor: &Editor, offset: usize) -> u16 {
    let b = editor.read_range(offset, 2);
    if b.len() < 2 { return 0; }
    u16::from_le_bytes([b[0], b[1]])
}

fn read_u32_le(editor: &Editor, offset: usize) -> u32 {
    let b = editor.read_range(offset, 4);
    if b.len() < 4 { return 0; }
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn read_u64_le(editor: &Editor, offset: usize) -> u64 {
    let b = editor.read_range(offset, 8);
    if b.len() < 8 { return 0; }
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn parse_elf(editor: &Editor) -> HeaderInfo {
    let mut fields = Vec::new();

    fields.push(("Magic".into(), "7F 45 4C 46 (ELF)".into()));

    let ei_class = editor.read_byte(4).unwrap_or(0);
    let class_str = match ei_class {
        1 => "32-bit (ELF32)",
        2 => "64-bit (ELF64)",
        _ => "Unknown",
    };
    fields.push(("Class".into(), class_str.into()));

    let ei_data = editor.read_byte(5).unwrap_or(0);
    let endian_str = match ei_data {
        1 => "Little-endian",
        2 => "Big-endian",
        _ => "Unknown",
    };
    fields.push(("Endianness".into(), endian_str.into()));

    let ei_version = editor.read_byte(6).unwrap_or(0);
    fields.push(("ELF Version".into(), format!("{}", ei_version)));

    let ei_osabi = editor.read_byte(7).unwrap_or(0);
    let osabi_str = match ei_osabi {
        0 => "UNIX System V",
        3 => "Linux",
        6 => "Solaris",
        9 => "FreeBSD",
        _ => "Other",
    };
    fields.push(("OS/ABI".into(), format!("{} ({})", osabi_str, ei_osabi)));

    let e_type = read_u16_le(editor, 16);
    let type_str = match e_type {
        1 => "Relocatable (ET_REL)",
        2 => "Executable (ET_EXEC)",
        3 => "Shared object (ET_DYN)",
        4 => "Core dump (ET_CORE)",
        _ => "Unknown",
    };
    fields.push(("Type".into(), type_str.into()));

    let e_machine = read_u16_le(editor, 18);
    let machine_str = match e_machine {
        0x03 => "x86",
        0x28 => "ARM",
        0x3E => "x86-64",
        0xB7 => "AArch64",
        0xF3 => "RISC-V",
        _ => "Other",
    };
    fields.push(("Machine".into(), format!("{} (0x{:X})", machine_str, e_machine)));

    if ei_class == 2 {
        // 64-bit
        let entry = read_u64_le(editor, 24);
        fields.push(("Entry point".into(), format!("0x{:016X}", entry)));

        let ph_off = read_u64_le(editor, 32);
        fields.push(("Program header offset".into(), format!("0x{:X}", ph_off)));

        let sh_off = read_u64_le(editor, 40);
        fields.push(("Section header offset".into(), format!("0x{:X}", sh_off)));

        let ph_num = read_u16_le(editor, 56);
        fields.push(("Program headers".into(), format!("{}", ph_num)));

        let sh_num = read_u16_le(editor, 60);
        fields.push(("Section headers".into(), format!("{}", sh_num)));
    } else if ei_class == 1 {
        // 32-bit
        let entry = read_u32_le(editor, 24);
        fields.push(("Entry point".into(), format!("0x{:08X}", entry)));

        let ph_off = read_u32_le(editor, 28);
        fields.push(("Program header offset".into(), format!("0x{:X}", ph_off)));

        let sh_off = read_u32_le(editor, 32);
        fields.push(("Section header offset".into(), format!("0x{:X}", sh_off)));

        let ph_num = read_u16_le(editor, 42);
        fields.push(("Program headers".into(), format!("{}", ph_num)));

        let sh_num = read_u16_le(editor, 48);
        fields.push(("Section headers".into(), format!("{}", sh_num)));
    }

    fields.push(("File size".into(), format_size(editor.file_size())));

    HeaderInfo {
        format_name: "ELF Binary".into(),
        fields,
    }
}

fn parse_gguf(editor: &Editor) -> HeaderInfo {
    let mut fields = Vec::new();

    fields.push(("Magic".into(), "47 47 55 46 (GGUF)".into()));

    let version = read_u32_le(editor, 4);
    fields.push(("Version".into(), format!("{}", version)));

    let tensor_count = read_u64_le(editor, 8);
    fields.push(("Tensor count".into(), format!("{}", tensor_count)));

    let metadata_kv_count = read_u64_le(editor, 16);
    fields.push(("Metadata KV count".into(), format!("{}", metadata_kv_count)));

    // Parse a few metadata KV pairs (best-effort, GGUF v3 format)
    // GGUF KV: key_len (u64), key (utf8), value_type (u32), value...
    let mut offset = 24usize;
    let max_kv = metadata_kv_count.min(20) as usize; // Show at most 20

    for _ in 0..max_kv {
        if offset + 8 > editor.file_size() {
            break;
        }
        let key_len = read_u64_le(editor, offset) as usize;
        offset += 8;

        if key_len > 256 || offset + key_len > editor.file_size() {
            break;
        }
        let key_bytes = editor.read_range(offset, key_len);
        let key = String::from_utf8_lossy(&key_bytes).to_string();
        offset += key_len;

        if offset + 4 > editor.file_size() {
            break;
        }
        let value_type = read_u32_le(editor, offset);
        offset += 4;

        let value_str = match value_type {
            // UINT8
            0 => {
                let v = editor.read_byte(offset).unwrap_or(0);
                offset += 1;
                format!("{}", v)
            }
            // INT8
            1 => {
                let v = editor.read_byte(offset).unwrap_or(0) as i8;
                offset += 1;
                format!("{}", v)
            }
            // UINT16
            2 => {
                let v = read_u16_le(editor, offset);
                offset += 2;
                format!("{}", v)
            }
            // INT16
            3 => {
                let v = read_u16_le(editor, offset) as i16;
                offset += 2;
                format!("{}", v)
            }
            // UINT32
            4 => {
                let v = read_u32_le(editor, offset);
                offset += 4;
                format!("{}", v)
            }
            // INT32
            5 => {
                let v = read_u32_le(editor, offset) as i32;
                offset += 4;
                format!("{}", v)
            }
            // FLOAT32
            6 => {
                let bits = read_u32_le(editor, offset);
                offset += 4;
                format!("{}", f32::from_bits(bits))
            }
            // BOOL
            7 => {
                let v = editor.read_byte(offset).unwrap_or(0);
                offset += 1;
                if v != 0 { "true".into() } else { "false".into() }
            }
            // STRING
            8 => {
                if offset + 8 > editor.file_size() {
                    break;
                }
                let slen = read_u64_le(editor, offset) as usize;
                offset += 8;
                if slen > 1024 || offset + slen > editor.file_size() {
                    let truncated = editor.read_range(offset, slen.min(64));
                    let s = String::from_utf8_lossy(&truncated);
                    offset += slen.min(editor.file_size() - offset);
                    format!("\"{}...\" (len={})", s, slen)
                } else {
                    let sbytes = editor.read_range(offset, slen);
                    offset += slen;
                    let s = String::from_utf8_lossy(&sbytes);
                    if s.len() > 80 {
                        format!("\"{}...\"", &s[..77])
                    } else {
                        format!("\"{}\"", s)
                    }
                }
            }
            // UINT64
            10 => {
                let v = read_u64_le(editor, offset);
                offset += 8;
                format!("{}", v)
            }
            // INT64
            11 => {
                let v = read_u64_le(editor, offset) as i64;
                offset += 8;
                format!("{}", v)
            }
            // FLOAT64
            12 => {
                let bits = read_u64_le(editor, offset);
                offset += 8;
                format!("{}", f64::from_bits(bits))
            }
            // ARRAY (9) and others — skip
            _ => {
                // Can't reliably parse arrays without recursion; stop here
                fields.push((key, format!("<type {} — skipped>", value_type)));
                break;
            }
        };

        fields.push((key, value_str));
    }

    fields.push(("File size".into(), format_size(editor.file_size())));

    HeaderInfo {
        format_name: "GGUF Model".into(),
        fields,
    }
}

fn parse_pe(editor: &Editor) -> HeaderInfo {
    let mut fields = Vec::new();
    fields.push(("Magic".into(), "4D 5A (MZ / PE)".into()));

    let pe_offset = read_u32_le(editor, 0x3C) as usize;
    if pe_offset + 4 <= editor.file_size() {
        let pe_sig = editor.read_range(pe_offset, 4);
        if pe_sig == [b'P', b'E', 0, 0] {
            fields.push(("PE Signature".into(), format!("at 0x{:X}", pe_offset)));
            let machine = read_u16_le(editor, pe_offset + 4);
            let machine_str = match machine {
                0x14c => "i386",
                0x8664 => "x86-64",
                0xAA64 => "AArch64",
                _ => "Other",
            };
            fields.push(("Machine".into(), format!("{} (0x{:X})", machine_str, machine)));

            let num_sections = read_u16_le(editor, pe_offset + 6);
            fields.push(("Sections".into(), format!("{}", num_sections)));
        }
    }

    fields.push(("File size".into(), format_size(editor.file_size())));

    HeaderInfo {
        format_name: "PE Executable".into(),
        fields,
    }
}

fn parse_png(editor: &Editor) -> HeaderInfo {
    let mut fields = Vec::new();
    fields.push(("Magic".into(), "89 50 4E 47 (PNG)".into()));

    // IHDR chunk at offset 8
    if editor.file_size() >= 29 {
        let width = read_u32_le(editor, 16).swap_bytes(); // PNG is big-endian
        let height = read_u32_le(editor, 20).swap_bytes();
        let bit_depth = editor.read_byte(24).unwrap_or(0);
        let color_type = editor.read_byte(25).unwrap_or(0);
        let ct_str = match color_type {
            0 => "Grayscale",
            2 => "RGB",
            3 => "Indexed",
            4 => "Grayscale+Alpha",
            6 => "RGBA",
            _ => "Unknown",
        };
        fields.push(("Dimensions".into(), format!("{}x{}", width, height)));
        fields.push(("Bit depth".into(), format!("{}", bit_depth)));
        fields.push(("Color type".into(), format!("{} ({})", ct_str, color_type)));
    }

    fields.push(("File size".into(), format_size(editor.file_size())));

    HeaderInfo {
        format_name: "PNG Image".into(),
        fields,
    }
}

fn parse_generic(editor: &Editor) -> HeaderInfo {
    let mut fields = Vec::new();

    // Show first 16 bytes as hex
    let head = editor.read_range(0, 16);
    let hex_str: Vec<String> = head.iter().map(|b| format!("{:02X}", b)).collect();
    fields.push(("First 16 bytes".into(), hex_str.join(" ")));

    // Try to identify as ASCII text
    let sample = editor.read_range(0, 256.min(editor.file_size()));
    let printable = sample.iter().filter(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()).count();
    let ratio = printable as f64 / sample.len() as f64;
    if ratio > 0.85 {
        fields.push(("Likely type".into(), "Text / ASCII".into()));
    } else {
        fields.push(("Likely type".into(), "Binary (unknown format)".into()));
    }

    fields.push(("File size".into(), format_size(editor.file_size())));

    HeaderInfo {
        format_name: "Unknown Format".into(),
        fields,
    }
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB ({} bytes)", bytes as f64 / 1024.0, bytes)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB ({} bytes)", bytes as f64 / (1024.0 * 1024.0), bytes)
    } else {
        format!("{:.2} GiB ({} bytes)", bytes as f64 / (1024.0 * 1024.0 * 1024.0), bytes)
    }
}
