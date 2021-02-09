use anyhow::Result;
use structopt::StructOpt;

use mapping_tools::*;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    verbosity: usize,

    #[structopt(subcommand)]
    subcommand: Subcommand,
}

#[derive(Debug, StructOpt)]
enum Subcommand {
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

impl Subcommand {
    pub fn run(self) -> Result<()> {
        use Subcommand::*;
        match self {
            CopyHitsounds { opts } => mapping_tools::copy_hitsounds_cmd(opts),
            ExtractMetadata { opts } => mapping_tools::extract_metadata(opts),
        }?;
        Ok(())
    }
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    stderrlog::new()
        .module(module_path!())
        .verbosity(opt.verbosity)
        .init()
        .unwrap();

    opt.subcommand.run()?;
    Ok(())
}
