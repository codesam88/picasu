use crate::process::hash::blake3_hasher;
use anyhow::Result;
use arrayvec::ArrayString;
use std::fs::File;

pub(crate) fn hash_task(file: File) -> Result<ArrayString<64>> {
    blake3_hasher(file)
}
