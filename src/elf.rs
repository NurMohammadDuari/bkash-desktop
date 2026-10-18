//! Minimal ELF64 little-endian reader, std-only.
//!
//! Scope is exactly what the Dart AOT workflow needs: locate the exported
//! `_kDart*Snapshot*` symbols in `.dynsym`/`.dynstr` and read their byte ranges.
//! No relocations, no DWARF, no disassembly.

use std::borrow::Cow;

/// Errors are plain strings: this module is a leaf with no crate coupling
/// beyond `std`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    TooShort,
    NotElf64,
    WrongEndianness,
    WrongMachine,
    Truncated(Cow<'static, str>),
    SymbolNotFound(String),
}

impl std::fmt::Display for ElfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElfError::TooShort => write!(f, "file shorter than e_ident"),
            ElfError::NotElf64 => write!(f, "not an ELF64 file"),
            ElfError::WrongEndianness => write!(f, "not little-endian"),
            ElfError::WrongMachine => write!(f, "not an AArch64 ELF (expected EM_AARCH64=183)"),
            ElfError::Truncated(what) => write!(f, "truncated while reading {what}"),
            ElfError::SymbolNotFound(n) => write!(f, "dynamic symbol {n} not found"),
        }
    }
}

impl std::error::Error for ElfError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub value: u64,
    pub size: u64,
}

/// One exported symbol and the bytes it spans inside the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub length: u32,
    pub kind: u32,
    pub version: u32,
    pub features: u32,
    /// The 16-byte snapshot hash. Dart stores it as hex ASCII, so this is the
    /// hash verbatim, not a re-encoding.
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct SymbolSection {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub bytes: Vec<u8>,
}

impl SymbolSection {
    /// Dart snapshot header: `magic:u32, length:u32, kind:u32, version:u32,
    /// features:u32`, followed by the 16-byte snapshot hash at offset 20.
    ///
    /// Verified against the real payload at `libapp.so+0x4ac0`:
    /// `f5f5dcdc f5e76400 00000000 03000000 00000000` + `1ce86630892e2dca9a85`.
    pub fn header(&self) -> Option<SnapshotHeader> {
        let head = self.bytes.get(..36)?;
        if u32::from_le_bytes(head[0..4].try_into().ok()?) != 0xdcdcf5f5 {
            return None;
        }
        Some(SnapshotHeader {
            length: u32::from_le_bytes(head[4..8].try_into().ok()?),
            kind: u32::from_le_bytes(head[8..12].try_into().ok()?),
            version: u32::from_le_bytes(head[12..16].try_into().ok()?),
            features: u32::from_le_bytes(head[16..20].try_into().ok()?),
            hash: String::from_utf8_lossy(&head[20..36])
                .trim_end_matches('\0')
                .to_string(),
        })
    }

    /// Convenience: the snapshot format version. `3` is the Dart 2.14 FullAOT
    /// layout, the only complete layout REA could decode for this artifact.
    pub fn snapshot_kind(&self) -> Option<u32> {
        self.header().map(|h| h.version)
    }
}

fn rd16(b: &[u8], off: usize) -> Result<u16, ElfError> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::TooShort)
}

fn rd32(b: &[u8], off: usize) -> Result<u32, ElfError> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::TooShort)
}

fn rd64(b: &[u8], off: usize) -> Result<u64, ElfError> {
    b.get(off..off + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::TooShort)
}

fn cstr(table: &[u8], index: u32) -> String {
    if index as usize >= table.len() {
        return String::new();
    }
    let rest = &table[index as usize..];
    let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// Parse `bytes` as ELF64-LE and return every dynamic symbol with a nonzero size.
pub fn dynamic_symbols(bytes: &[u8]) -> Result<Vec<Symbol>, ElfError> {
    if bytes.len() < 4 {
        return Err(ElfError::TooShort);
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err(ElfError::NotElf64);
    }
    if bytes.len() < 64 {
        return Err(ElfError::Truncated("ELF header".into()));
    }
    if bytes[5] != 1 {
        return Err(ElfError::WrongEndianness);
    }
    if rd16(bytes, 18)? != 183 {
        return Err(ElfError::WrongMachine);
    }

    let shoff = rd64(bytes, 0x28)?;
    let shentsize = rd16(bytes, 0x3a)? as usize;
    let shnum = rd16(bytes, 0x3c)? as usize;
    let shstrndx = rd16(bytes, 0x3e)? as usize;

    if shnum == 0 || shentsize < 64 {
        return Err(ElfError::Truncated("section headers".into()));
    }

    let sh = |i: usize| -> Result<usize, ElfError> {
        let base = shoff as usize + i * shentsize;
        if base + 64 > bytes.len() {
            return Err(ElfError::Truncated("section header table".into()));
        }
        Ok(base)
    };

    let strtab_base = sh(shstrndx)?;
    let shstr_off = rd64(bytes, strtab_base + 0x18)?;
    let shstr_len = rd64(bytes, strtab_base + 0x20)?;
    let shstrtab = bytes
        .get(shstr_off as usize..(shstr_off + shstr_len) as usize)
        .ok_or(ElfError::Truncated(".shstrtab".into()))?;

    // Locate .dynsym / .dynstr by name, then walk the symbol table.
    let mut sym_idx = None;
    let mut dynstr_idx = None;
    for i in 0..shnum {
        let b = sh(i)?;
        let name = cstr(shstrtab, rd32(bytes, b)?);
        match name.as_str() {
            ".dynsym" => sym_idx = Some(i),
            ".dynstr" => dynstr_idx = Some(i),
            _ => {}
        }
    }
    let (sym_idx, dynstr_idx) = match (sym_idx, dynstr_idx) {
        (Some(a), Some(b)) => (a, b),
        _ => return Err(ElfError::Truncated(".dynsym/.dynstr".into())),
    };

    let symtab = sh(sym_idx)?;
    let sym_off = rd64(bytes, symtab + 0x18)?;
    let sym_size = rd64(bytes, symtab + 0x20)?;
    let sym_entsize = rd64(bytes, symtab + 0x38)? as usize;

    let ds = sh(dynstr_idx)?;
    let ds_off = rd64(bytes, ds + 0x18)?;
    let ds_len = rd64(bytes, ds + 0x20)?;
    let dynstr = bytes
        .get(ds_off as usize..(ds_off + ds_len) as usize)
        .ok_or(ElfError::Truncated(".dynstr".into()))?;

    let entsize = if sym_entsize == 0 { 24 } else { sym_entsize };
    let count = (sym_size as usize) / entsize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = sym_off as usize + i * entsize;
        let name_idx = rd32(bytes, base)?;
        let info = *bytes
            .get(base + 4)
            .ok_or(ElfError::Truncated("sym entry".into()))?;
        let shndx = rd16(bytes, base + 6)?;
        let value = rd64(bytes, base + 8)?;
        let size = rd64(bytes, base + 16)?;
        let bind = info >> 4;
        // STB_GLOBAL(1)/STB_WEAK(2)/STB_GNU_UNIQUE(10), SHN_UNDEF excluded.
        if (1..=2).contains(&bind) && shndx != 0 && size > 0 {
            out.push(Symbol {
                name: cstr(dynstr, name_idx),
                value,
                size,
            });
        }
    }
    Ok(out)
}

/// Read the file bytes spanned by a sized dynamic symbol.
pub fn symbol_bytes(bytes: &[u8], sym: &Symbol) -> Result<Vec<u8>, ElfError> {
    let start = sym.value as usize;
    let end = start
        .checked_add(sym.size as usize)
        .ok_or(ElfError::Truncated(sym.name.clone().into()))?;
    bytes
        .get(start..end)
        .map(|s| s.to_vec())
        .ok_or(ElfError::Truncated(sym.name.clone().into()))
}

/// Find the four Dart snapshot symbols and return their file ranges.
pub fn dart_snapshot_sections(bytes: &[u8]) -> Result<Vec<SymbolSection>, ElfError> {
    let wanted = [
        "_kDartVmSnapshotInstructions",
        "_kDartIsolateSnapshotInstructions",
        "_kDartVmSnapshotData",
        "_kDartIsolateSnapshotData",
    ];
    let syms = dynamic_symbols(bytes)?;
    let mut out = Vec::new();
    for name in wanted {
        let sym = syms
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| ElfError::SymbolNotFound(name.to_string()))?;
        out.push(SymbolSection {
            name: sym.name.clone(),
            offset: sym.value,
            size: sym.size,
            bytes: symbol_bytes(bytes, sym)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Build a minimal but structurally valid ELF64-LE aarch64 object with one
    /// `.dynsym` / `.dynstr` pair exporting `sized_symbol`.
    fn synth_elf(_symbol: &str, value: u64, size: u64, payload: &[u8]) -> Vec<u8> {
        const SHNUM: usize = 5; // null, .dynstr, .dynsym, .shstrtab, .text
        let shentsize = 64usize;
        let shoff = 0x1000usize;

        let dynstr = b"\0sized_symbol\0";
        let dynstr_off = 0x200usize;
        let shstr = b"\0.shstrtab\0.dynsym\0.dynstr\0.text\0";
        let shstr_off = 0x300usize;
        let text_off = 0x400usize;

        let symtab = vec![0u8; 24 * 2];
        let dynsym_off = 0x500usize;

        let mut buf = vec![0u8; shoff + SHNUM * shentsize];
        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[6] = 1; // EV_CURRENT
        buf[16..18].copy_from_slice(&1u16.to_le_bytes()); // ET_REL
        buf[18..20].copy_from_slice(&183u16.to_le_bytes()); // EM_AARCH64
        buf[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        buf[0x34..0x36].copy_from_slice(&64u16.to_le_bytes()); // ehsize
        buf[0x3a..0x3c].copy_from_slice(&(shentsize as u16).to_le_bytes());
        buf[0x3c..0x3e].copy_from_slice(&(SHNUM as u16).to_le_bytes());
        buf[0x3e..0x40].copy_from_slice(&3u16.to_le_bytes()); // e_shstrndx -> .shstrtab

        let mut put =
            |i: usize, name: u32, ty: u32, off: u64, size: u64, link: u32, entsize: u64| {
                let b = shoff + i * shentsize;
                buf[b..b + 4].copy_from_slice(&name.to_le_bytes());
                buf[b + 4..b + 8].copy_from_slice(&ty.to_le_bytes());
                buf[b + 0x18..b + 0x20].copy_from_slice(&off.to_le_bytes());
                buf[b + 0x20..b + 0x28].copy_from_slice(&size.to_le_bytes());
                buf[b + 0x28..b + 0x2c].copy_from_slice(&link.to_le_bytes());
                buf[b + 0x38..b + 0x40].copy_from_slice(&entsize.to_le_bytes());
            };

        put(1, 19, 3, dynstr_off as u64, dynstr.len() as u64, 0, 0);
        put(2, 11, 11, dynsym_off as u64, symtab.len() as u64, 1, 24);
        put(3, 1, 3, shstr_off as u64, shstr.len() as u64, 0, 0);
        put(4, 27, 1, text_off as u64, payload.len() as u64, 0, 0);

        let need = dynstr_off.max(shstr_off).max(dynsym_off).max(text_off)
            + dynstr
                .len()
                .max(shstr.len())
                .max(symtab.len())
                .max(payload.len());
        buf.resize(need.max(buf.len()), 0);
        buf[dynstr_off..dynstr_off + dynstr.len()].copy_from_slice(dynstr);
        buf[shstr_off..shstr_off + shstr.len()].copy_from_slice(shstr);
        buf[text_off..text_off + payload.len()].copy_from_slice(payload);

        // symbol 0: null. symbol 1: sized_symbol, GLOBAL, SHN_ABS(0xfff1).
        let s1 = dynsym_off + 24;
        buf[s1..s1 + 4].copy_from_slice(&1u32.to_le_bytes());
        buf[s1 + 4] = (1 << 4) | 2; // STB_GLOBAL | STT_FUNC
        buf[s1 + 6..s1 + 8].copy_from_slice(&0xfff1u16.to_le_bytes());
        buf[s1 + 8..s1 + 16].copy_from_slice(&value.to_le_bytes());
        buf[s1 + 16..s1 + 24].copy_from_slice(&size.to_le_bytes());
        buf
    }

    #[test]
    fn parses_sized_dynamic_symbol() {
        // Dart magic 0xdcdcf5f5 in little-endian byte order.
        // magic, length=0x4177, kind=0, version=3, then the 16-byte hash.
        let mut payload = vec![
            0xf5u8, 0xf5, 0xdc, 0xdc, 0x77, 0x41, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0,
        ];
        payload.extend_from_slice(&[0u8; 4]);
        payload.extend_from_slice(b"1ce86630892e2dca");
        let elf = synth_elf("sized_symbol", 0x400, payload.len() as u64, &payload);
        let syms = dynamic_symbols(&elf).expect("parse");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "sized_symbol");
        assert_eq!(syms[0].value, 0x400);
        assert_eq!(syms[0].size, payload.len() as u64);
        let sec = SymbolSection {
            name: syms[0].name.clone(),
            offset: syms[0].value,
            size: syms[0].size,
            bytes: symbol_bytes(&elf, &syms[0]).expect("bytes"),
        };
        assert_eq!(sec.bytes.len(), 36, "payload slice");
        let h = sec.header().expect("header");
        assert_eq!(h.length, 0x4177);
        assert_eq!(h.kind, 0);
        assert_eq!(h.version, 3);
        assert_eq!(h.features, 0);
        assert_eq!(h.hash, "1ce86630892e2dca");
        assert_eq!(sec.snapshot_kind(), Some(3));
    }

    #[test]
    fn rejects_non_elf_and_wrong_machine() {
        assert_eq!(
            dynamic_symbols(b"not an elf at all, padded out to length"),
            Err(ElfError::NotElf64)
        );
        let mut elf = synth_elf("s", 0x400, 8, &[0u8; 8]);
        elf[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        assert_eq!(dynamic_symbols(&elf), Err(ElfError::WrongMachine));
    }

    #[test]
    fn missing_symbol_is_reported_not_guessed() {
        let elf = synth_elf("other", 0x400, 8, &[0u8; 8]);
        let err = dart_snapshot_sections(&elf).unwrap_err();
        assert!(matches!(err, ElfError::SymbolNotFound(_)), "got {err:?}");
    }

    #[test]
    fn snapshot_kind_is_none_without_magic() {
        let sec = SymbolSection {
            name: "x".into(),
            offset: 0,
            size: 8,
            bytes: vec![0, 1, 2, 3, 4, 5, 6, 7],
        };
        assert_eq!(sec.snapshot_kind(), None);
    }
}
