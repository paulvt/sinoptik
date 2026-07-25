use std::error::Error;
use vergen_git2::{Build, Emitter, Git2};

fn main() -> Result<(), Box<dyn Error>> {
    let build = Build::builder().build_timestamp(true).build();
    let git = Git2::builder().commit_timestamp(true).sha(true).build();

    // Generate the `cargo:` instructions to fill the appropriate environment variables.
    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&git)?
        .emit()?;

    Ok(())
}
