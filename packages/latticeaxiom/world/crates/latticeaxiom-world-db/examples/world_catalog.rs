//! Inspect existing saved-world metadata without loading voxel payloads.

use std::{io, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: world_catalog <existing worlds.redb>")?,
    );
    if !path.is_file() {
        return Err("the saved-world database does not exist".into());
    }
    let disk = latticeaxiom_world_db::DiskWorldStore::open(&path)?;
    serde_json::to_writer_pretty(io::stdout().lock(), &disk.entries()?)?;
    Ok(())
}
