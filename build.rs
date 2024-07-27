use std::error::Error;
use vergen_git2::{BuildBuilder, Emitter, Git2Builder};

fn main() -> Result<(), Box<dyn Error>> {
    // Generate the `cargo:` instructions to fill the appropriate environment variables.
    let build = &BuildBuilder::all_build()?;
    let git2 = &Git2Builder::all_git()?;

    Emitter::default()
        .add_instructions(build)?
        .add_instructions(git2)?
        .emit()?;

    Ok(())
}
