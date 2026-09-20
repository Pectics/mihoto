use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine};
use colored::Colorize;
use flate2::read::GzDecoder;

/// Creates the parent directory for a given path if it does not exist.
///
/// # Arguments
///
/// * `path` - A string slice that holds the path for which the parent directory should be created.
pub fn create_parent_dir(path: &Path) -> Result<()> {
    // let parent_dir = Path::new(path)
    let parent_dir = path
        .parent()
        .with_context(|| format!("parent directory of `{}` invalid", path.to_string_lossy()))?;
    if !parent_dir.exists() {
        fs::create_dir_all(parent_dir)?;
    }
    Ok(())
}

pub fn delete_file(path: &str, prefix: impl std::fmt::Display) -> Result<()> {
    // Delete file if exists
    if Path::new(path).exists() {
        fs::remove_file(path).map(|_| {
            println!("{} Removed {}", prefix, path.underline().yellow());
        })?;
    }
    Ok(())
}

pub fn extract_gzip(from_path: &Path, to_path: &str, prefix: impl std::fmt::Display) -> Result<()> {
    // Create parent directory for extraction dest if not exists
    create_parent_dir(Path::new(to_path))?;

    // Extract gzip file
    let mut archive = GzDecoder::new(File::open(from_path)?);
    let mut file = File::create(to_path)?;
    io::copy(&mut archive, &mut file)?;
    // fs::remove_file(gzip_path)?;
    println!("{} Extracted to {}", prefix, to_path.underline().yellow());
    Ok(())
}

/// Try and decode a base64 encoded file in place.
///
/// Decodes the base64 encoded content of a file in place and writes the decoded content back to the
/// file. If the file does not contain base64 encoded content, maintains the file as is.
///
/// # Arguments
///
/// * `filepath` - Path to the file to decode base64 content in place.
pub fn try_decode_base64_file_inplace(filepath: &str) -> Result<()> {
    // Open the file for reading and writing
    let mut file = File::options().read(true).write(true).open(filepath)?;
    let mut base64_buf = Vec::new();

    // Read the file content into the buffer
    file.read_to_end(&mut base64_buf)?;

    // Try to decode the base64 content
    match BASE64_STANDARD.decode(&base64_buf) {
        Ok(decoded_bytes) => {
            // Truncate the file and seek to the beginning
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;

            // Write the decoded bytes back to the file
            let mut writer = BufWriter::new(&file);
            writer.write_all(&decoded_bytes)?;
        }
        Err(_) => {
            // If decoding fails, do nothing and return Ok
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn file_helpers_preserve_existing_behavior() -> Result<()> {
        let dir = tempdir()?;
        let nested = dir.path().join("nested/dir/file.txt");
        create_parent_dir(&nested)?;
        assert!(nested.parent().unwrap().exists());

        fs::write(&nested, "test content")?;
        delete_file(nested.to_str().unwrap(), "prefix")?;
        assert!(!nested.exists());
        delete_file(nested.to_str().unwrap(), "prefix")?;

        let encoded = dir.path().join("encoded");
        fs::write(
            &encoded,
            base64::engine::general_purpose::STANDARD.encode("decoded"),
        )?;
        try_decode_base64_file_inplace(encoded.to_str().unwrap())?;
        assert_eq!(fs::read_to_string(encoded)?, "decoded");
        Ok(())
    }

    #[test]
    fn invalid_base64_is_left_unchanged() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("plain");
        fs::write(&path, "not valid base64!!!")?;
        try_decode_base64_file_inplace(path.to_str().unwrap())?;
        assert_eq!(fs::read_to_string(path)?, "not valid base64!!!");
        Ok(())
    }

    #[test]
    fn gzip_is_extracted() -> Result<()> {
        use flate2::{write::GzEncoder, Compression};

        let dir = tempdir()?;
        let archive = dir.path().join("test.gz");
        let output = dir.path().join("output");
        let mut encoder = GzEncoder::new(File::create(&archive)?, Compression::default());
        encoder.write_all(b"test content")?;
        encoder.finish()?;
        extract_gzip(&archive, output.to_str().unwrap(), "prefix")?;
        assert_eq!(fs::read_to_string(output)?, "test content");
        Ok(())
    }
}
