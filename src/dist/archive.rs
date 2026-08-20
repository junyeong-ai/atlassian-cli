//! Reading one file out of a release archive.

use std::io::Read;

use flate2::read::GzDecoder;

use super::DistError;

/// Read the named member of a `.tar.gz` held in memory.
///
/// Nothing is unpacked. The caller names the file it wants and gets its bytes,
/// so no path chosen by the archive ever reaches the filesystem and the
/// question of a `../` escaping the destination never arises.
pub fn read_from_tar_gz(archive: &[u8], member: &str) -> Result<Option<Vec<u8>>, DistError> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    let entries = tar
        .entries()
        .map_err(|e| DistError::Integrity(format!("the archive could not be read: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| DistError::Integrity(format!("the archive is truncated: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| DistError::Integrity(format!("the archive holds a bad path: {e}")))?
            .to_string_lossy()
            .into_owned();
        if path != member {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| {
            DistError::Integrity(format!("reading {member} out of the archive: {e}"))
        })?;
        return Ok(Some(bytes));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn the_named_member_comes_back_and_the_others_do_not() {
        let archive = tar_gz(&[("atlassian-cli", b"binary"), ("README", b"prose")]);
        assert_eq!(
            read_from_tar_gz(&archive, "atlassian-cli").unwrap(),
            Some(b"binary".to_vec())
        );
        assert_eq!(
            read_from_tar_gz(&archive, "atlassian-cli.exe").unwrap(),
            None
        );
    }

    #[test]
    fn a_member_is_matched_on_its_whole_path_and_never_written_out() {
        // The archive's own paths decide only what a caller can ask for. A
        // member somewhere else in the tree is not the one that was named, and
        // no path from the archive reaches the filesystem either way.
        let archive = tar_gz(&[("nested/atlassian-cli", b"binary")]);
        assert_eq!(read_from_tar_gz(&archive, "atlassian-cli").unwrap(), None);
        assert_eq!(
            read_from_tar_gz(&archive, "nested/atlassian-cli").unwrap(),
            Some(b"binary".to_vec())
        );
    }

    #[test]
    fn bytes_that_are_not_an_archive_fail_rather_than_read_as_empty() {
        let err = read_from_tar_gz(b"not a gzip stream at all", "atlassian-cli").unwrap_err();
        assert!(matches!(err, DistError::Integrity(_)), "{err}");
    }
}
