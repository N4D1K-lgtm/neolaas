use vergen_gitcl::{BuildBuilder, CargoBuilder, Emitter, GitclBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = BuildBuilder::all_build()?;
    let cargo = CargoBuilder::all_cargo()?;
    let rustc = RustcBuilder::all_rustc()?;

    // Try to get git info from the repository first
    let gitcl = GitclBuilder::all_git();

    let mut emitter = Emitter::default();
    emitter
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?;

    // If git info is available, use it; otherwise fall back to env vars
    if let Ok(git) = gitcl {
        emitter.add_instructions(&git)?;
    } else {
        // Fall back to environment variables (set by Docker build args or CI)
        println!(
            "cargo::rustc-env=VERGEN_GIT_SHA={}",
            std::env::var("VERGEN_GIT_SHA").unwrap_or_else(|_| "unknown".to_string())
        );
        println!(
            "cargo::rustc-env=VERGEN_GIT_BRANCH={}",
            std::env::var("VERGEN_GIT_BRANCH").unwrap_or_else(|_| "unknown".to_string())
        );
        println!(
            "cargo::rustc-env=VERGEN_GIT_COMMIT_TIMESTAMP={}",
            std::env::var("VERGEN_GIT_COMMIT_TIMESTAMP").unwrap_or_else(|_| "unknown".to_string())
        );
        println!(
            "cargo::rustc-env=VERGEN_GIT_DIRTY={}",
            std::env::var("VERGEN_GIT_DIRTY").unwrap_or_else(|_| "false".to_string())
        );
    }

    emitter.emit()?;

    Ok(())
}
