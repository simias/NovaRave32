#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;

mod audio;
mod model;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "multitool",
    version = "1.0",
    about = "A multitool with various commands"
)]
struct Cli {
    /// Enables verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Converts 3D model files into NR3D files
    Model {
        /// The model file to process
        model_file: PathBuf,

        /// Keep normals in output
        #[arg(long)]
        keep_normals: bool,

        /// Which mesh index to export
        #[arg(long, default_value_t = 0)]
        mesh: usize,

        /// Scale to apply to the model (will be computed based on the bounding box if not)
        #[arg(long)]
        scale: Option<f32>,

        /// Do not recenter the model. Only scaling will be applied to the vertex coordinates.
        #[arg(long, default_value_t = false)]
        no_recenter: bool,

        /// NR3D file to dump the converted mesh
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Converts audio files into NRAD files
    Audio {
        /// The audio file to process
        input_file: PathBuf,

        /// Sample rate for the output in Hz (defaults to input sample rate). Max 48000Hz.
        #[arg(short, long)]
        sample_rate: Option<u32>,

        /// Which input channel to use (output files are always mono). If not set, all available
        /// channels will be averaged.
        #[arg(long, short)]
        channel: Option<usize>,

        /// NRAD file to dump the converted mesh
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    match cli.command {
        Commands::Model {
            model_file,
            mesh,
            keep_normals,
            scale,
            no_recenter,
            output,
        } => {
            let model = model::Model::options()
                .keep_normals(keep_normals)
                .mesh(mesh)
                .scale(scale)
                .recenter(!no_recenter)
                .load(&model_file)?;

            info!("Loaded mesh with {} triangles", model.triangle_count());

            if let Some(out) = output {
                info!("Dumping model to {}", out.display());
                let mut out = BufWriter::new(File::create(out)?);
                model.dump_nr3d(&mut out)?
            }
        }
        Commands::Audio {
            input_file,
            sample_rate,
            channel,
            output,
        } => {
            let buf = audio::AudioBuffer::from_path(input_file, channel)?;

            info!(
                "Input sample rate: {}Hz ({} samples total)",
                buf.sample_rate(),
                buf.samples().len()
            );

            if let Some(sample_rate) = sample_rate {
                if sample_rate != buf.sample_rate() {
                    todo!("Resample {} -> {}!", buf.sample_rate(), sample_rate);
                }
            }

            if let Some(out) = output {
                info!("Dumping audio to {}", out.display());
                let mut out = BufWriter::new(File::create(out)?);
                buf.dump_nrad(&mut out)?
            }
        }
    }

    Ok(())
}
