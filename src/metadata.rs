use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use libosu::prelude::*;

#[derive(Serialize, Deserialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub title_unicode: Option<String>,
    pub artist: Option<String>,
    pub artist_unicode: Option<String>,

    pub tags: Vec<String>,
}

#[derive(Debug, StructOpt)]
pub struct ExtractMetadataOpts {
    /// The .osu file to read metadata from.
    pub file: PathBuf,
}

pub fn extract_metadata(opts: ExtractMetadataOpts) -> Result<()> {
    let file = File::open(&opts.file)?;
    let beatmap = Beatmap::parse(file)?;

    let metadata = Metadata {
        title: Some(beatmap.title.clone()),
        title_unicode: Some(beatmap.title_unicode.clone()),
        artist: Some(beatmap.artist.clone()),
        artist_unicode: Some(beatmap.artist_unicode.clone()),
        tags: beatmap.tags.clone(),
    };

    let output = toml::to_string(&metadata)?;
    println!("{}", output);
    Ok(())
}

#[derive(Debug, StructOpt)]
pub struct ApplyMetadataOpts {
    /// The list of .osu files to apply the input metadata to.
    pub files: Vec<PathBuf>,
}
