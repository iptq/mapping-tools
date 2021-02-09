use anyhow::Result;
use structopt::StructOpt;

use mapping_tools::*;

#[derive(Debug, StructOpt)]
enum Opt {
    /// Copy hitsounds from one map to another.
    #[structopt(name = "copy-hitsounds")]
    CopyHitsounds {
        #[structopt(flatten)]
        opts: CopyHitsoundOpts,
    },

    /// Extracts metadata from the map and prints to stdout.
    #[structopt(name = "extract-metadata")]
    ExtractMetadata {
        #[structopt(flatten)]
        opts: ExtractMetadataOpts,
    },
}

impl Opt {
    pub fn run(self) -> Result<()> {
        use Opt::*;
        match self {
            CopyHitsounds { opts } => mapping_tools::copy_hitsounds(opts),
            ExtractMetadata { opts } => mapping_tools::extract_metadata(opts),
        }?;
        Ok(())
    }
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    opt.run()?;
    Ok(())
}
