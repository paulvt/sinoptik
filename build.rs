use std::error::Error;
use vergen_git2::{BuildBuilder, Emitter, Git2Builder};

fn main() -> Result<(), Box<dyn Error>> {
    // Generate the `cargo:` instructions to fill the appropriate environment variables.
    Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&Git2Builder::all_git()?)?
        .emit()?;

    Ok(())
}
