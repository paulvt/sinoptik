use std::error::Error;
use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn Error>> {
    // Generate the `cargo:` instructions to fill the appropriate environment variables.
    EmitBuilder::builder().all_build().all_git().emit()?;

    Ok(())
}
