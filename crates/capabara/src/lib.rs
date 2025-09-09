use std::{
    fs,
    io::{Cursor, Read as _},
    path::Path,
};

use anyhow::{Context, Result};
use object::{Object, ObjectSymbol};

use crate::symbol::Symbol;

pub mod demangle;
pub mod print;
pub mod symbol;

/// Extact symbols from an executable or an .rlib.
pub fn extract_symbols(binary_path: &Path) -> Result<Vec<Symbol>> {
    let file_bytes = fs::read(binary_path)
        .with_context(|| format!("Failed to read {}", binary_path.display()))?;

    // Check if it's an ar archive (rlib files are ar archives)
    let is_ar_archive = file_bytes.starts_with(b"!<arch>\n");

    let mut symbols = Vec::new();

    if is_ar_archive {
        // .rlib
        let mut archive = ar::Archive::new(Cursor::new(file_bytes));

        // Extract and process each object file in the archive
        while let Some(entry_result) = archive.next_entry() {
            let mut entry = entry_result.context("Failed to read archive entry")?;

            // Skip non-object files (like metadata files)
            let header = entry.header();
            let filename = String::from_utf8_lossy(header.identifier());

            if !filename.ends_with(".o") {
                continue;
            }

            // Read the object file data
            let mut obj_data = Vec::new();
            entry
                .read_to_end(&mut obj_data)
                .context("Failed to read object file from archive")?;

            // Parse the object file and extract symbols
            if let Ok(file) = object::File::parse(&*obj_data) {
                collect_file_symbols(&mut symbols, &file);
            } else {
                // TODO: Return error
            }
        }
    } else {
        // Assume an executable
        let file =
            object::File::parse(&*file_bytes).with_context(|| "Failed to parse binary file")?;
        collect_file_symbols(&mut symbols, &file);
    }

    Ok(symbols)
}

fn collect_file_symbols(all_symbols: &mut Vec<Symbol>, file: &object::File<'_>) {
    for symbol in file.symbols() {
        if let Ok(name) = symbol.name()
            && !name.is_empty()
        {
            all_symbols.push(Symbol::from_mangled(name.to_string()));
        }
    }
}
