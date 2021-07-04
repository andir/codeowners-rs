use std::rc::Rc;
use hubcaps::{Github};
use futures::stream::{StreamExt, TryStreamExt};
use anyhow::Context;

mod lib;
use lib::*;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .context("Failed to initialize logger")?;
    let github = Github::new(
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
        None,
    )?;
    let repo = github.repo("NixOS", "nixpkgs");

    let codeowners = Rc::new(parse(
        std::str::from_utf8(&std::fs::read("./CODEOWNERS").unwrap()).unwrap()
    ).unwrap());

    let pulls = repo.pulls();
    let stream = futures::stream::iter(
        pulls
            .list(&Default::default())
            .await?
            .into_iter()
    );
    stream
        .inspect(|pull| log::info!("Processing PR: {}", pull.number))
        .map(|pull| (pull.number, pulls.get(pull.number)))
        .flat_map(|(number, pr)| pr.iter_files().map_ok(move |file| (number, file)))
        .map_ok(|(number, diff)| (number, format!("/{}", diff.filename)))
        .try_for_each(|(number, path)| {
            let codeowners = codeowners.clone();
            async move {
                for owner in codeowners.matches(&path).into_iter().flatten() {
                    log::info!("PR {}: Ping {}, because of '{}'", number, owner, path);
                }
                Ok(())
            }
        })
        .await?;

    Ok(())
}
