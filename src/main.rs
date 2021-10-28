use std::rc::Rc;
use std::collections::{BTreeMap, HashMap};
use hubcaps::{Github, SortDirection, issues::{State, Sort}, pulls::PullListOptions};
use futures::stream::{StreamExt};
use anyhow::Context;

mod lib;
use lib::*;

// (pr -> (owner -> [files that matched]))
type PersistentState = BTreeMap<u64, HashMap<String, Vec<String>>>;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .context("Failed to initialize logger")?;

    let mut state: PersistentState = serde_json::from_reader(std::fs::File::open("state.json")?)?;

    let github = Github::new(
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
        None,
    )?;
    let repo = github.repo("NixOS", "nixpkgs");

    let codeowners = Rc::new(parse(
        std::str::from_utf8(&std::fs::read("./CODEOWNERS").unwrap()).unwrap()
    ).unwrap());

    let pulls = repo.pulls();
    let mut pulls_stream = pulls
        .iter(&{
            let mut builder = PullListOptions::builder();
            builder.state(State::Open);
            builder.sort(Sort::Updated);
            builder.direction(SortDirection::Desc);
            builder.build()
        });
    while let Some(pull) = pulls_stream.next().await.transpose()? {
        let number = pull.number;
        if state.contains_key(&number) {
            log::info!("(Skipping #{})", number);
            continue;
        }
        log::info!("Processing PR: #{}", number);
        
        let pr = pulls.get(number);
        let mut iter = pr.iter_files();
        let mut pinged = HashMap::<String, Vec<String>>::new();
        while let Some(diff) = iter.next().await.transpose()? {
            let path = format!("/{}", diff.filename);
            for owner in codeowners.matches(&path).into_iter().flatten() {
                log::info!("PR {}: Ping {}, because of '{}'", number, owner, path);
                match pinged.entry(owner.to_string()) {
                    std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(path.clone()),
                    std::collections::hash_map::Entry::Vacant(e) => {e.insert(vec![path.clone()]);},
                }
            }
        }
        state.insert(number, pinged);

        serde_json::to_writer_pretty(std::fs::File::create("state.json")?, &state)?;
    }
    
    Ok(())
}
