use globset::{Glob, GlobSet};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ParseError {
    #[error("Missing owner in input {input:?}")]
    MissingOwners { input: String },

    #[error("Failed to compile Glob pattern")]
    Glob(#[from] globset::Error),
}

/// Represents one Codeowner as either a GitHub handle via an Email address.
///
/// For now it is assumed that all those values that aren't GitHub handles are email addresses.
#[derive(Debug, PartialEq, Eq)]
pub enum Owner {
    Email(String),
    Handle(String),
}
impl Owner {
    pub fn parse(input: impl AsRef<str>) -> Owner {
        let input = input.as_ref();
        if input.starts_with('@') {
            Owner::Handle(input.to_string())
        } else {
            Owner::Email(input.to_string())
        }
    }
}

struct CodeownersGlob {
    glob: globset::Glob,
    absolute: bool,
}

/// Convert a Codeowners pattern into a glob pattern
fn pattern_to_glob(pattern: impl AsRef<str>) -> Result<CodeownersGlob, ParseError> {
    let mut pattern = pattern.as_ref();
    let mut absolute = false;

    // patterns that start with a / only match in the root
    if pattern.starts_with('/') {
        pattern = &pattern[1..];
        absolute = true;
    }

    let mut pattern = pattern.to_string();
    // If a path ends with / then include all subpaths as well
    if pattern.ends_with('/') {
        pattern = format!("{}**", pattern);
    }

    // for paths that aren't absolute but have a slash somewhere we must prefix them with **/ so
    // they match in any subdirectory.
    if !absolute && pattern.contains('/') {
        pattern = format!("**/{}", pattern);
    }

    Ok(CodeownersGlob {
        glob: globset::Glob::new(&pattern)?,
        absolute,
    })
}

/// Representation of one Codeowner pattern and the respective list of owners.
#[derive(Debug)]
pub struct Rule {
    pub pattern: String,
    pub owners: Vec<Owner>,
}

impl PartialEq for Rule {
    fn eq(&self, other: &Rule) -> bool {
        self.pattern == other.pattern && self.owners == other.owners
    }
}

impl Rule {
    pub fn parse(input: impl AsRef<str>) -> Result<Rule, ParseError> {
        let input = input.as_ref();
        // split in spaces and ignore multiple spaces
        let parts = input
            .split(' ')
            .filter(|part| !part.is_empty())
            .collect::<Vec<&str>>();
        if parts.len() < 2 {
            return Err(ParseError::MissingOwners {
                input: input.to_string(),
            });
        }
        let pattern = parts[0].to_string();
        let owners = parts[1..].iter().map(Owner::parse).collect();

        Ok(Rule { pattern, owners })
    }
}

/// The parsed representation of a GitHub Codeowners file
#[derive(Debug)]
pub struct Codeowners {
    pub rules: Vec<Rule>,
    globset: GlobSet,
}

impl PartialEq for Codeowners {
    fn eq(&self, other: &Codeowners) -> bool {
        self.rules == other.rules
    }
}

impl Codeowners {
    /// Match the given path against the set of rules and return the matching rules or None.
    pub fn matches<'a, T: AsRef<std::path::Path>>(&'a self, path: T) -> Option<&'a Rule> {
        // The Codeowners documentation states that the last rule in the file (the last one in our
        // vector) has the highest priority. This means we will go through all the rules in reverse
        // order.
        let matches = self
            .globset
            .matches_candidate(&globset::Candidate::new(&path));

        let index = matches.iter().rev().nth(0);
        index.map(|x| self.rules.get(*x)).flatten()
    }
}

/// Parse the given input as Codeowners file
pub fn parse(input: impl AsRef<str>) -> Result<Codeowners, ParseError> {
    let input = input.as_ref();
    let non_comment_lines_iterator = input
        .lines()
        // trim any whitespace from the start
        .map(|line| line.trim_start())
        // ignore empty lines
        .filter(|line| !line.is_empty())
        // ignore comments
        .filter(|line| !line.starts_with('#'));
    // map all the remaining lines into Rule instances
    let rules: Vec<_> = non_comment_lines_iterator
        .map(Rule::parse)
        .collect::<Result<Vec<_>, _>>()?;

    let mut builder = globset::GlobSetBuilder::new();
    for rule in rules.iter() {
        let glob = Glob::new(&rule.pattern)?;
        builder.add(glob);
    }
    let globset = builder.build()?;

    Ok(Codeowners { rules, globset })
}

#[cfg(test)]
mod tests {

    use super::{Owner::*, *};

    /// Test the example given in the [GitHub CODEOWNERS
    /// documentation](https://docs.github.com/en/github/creating-cloning-and-archiving-repositories/creating-a-repository-on-github/about-code-owners#codeowners-syntax).
    #[test]
    fn test_github_example() {
        let example = r#"# This is a comment.
# Each line is a file pattern followed by one or more owners.

# These owners will be the default owners for everything in
# the repo. Unless a later match takes precedence,
# @global-owner1 and @global-owner2 will be requested for
# review when someone opens a pull request.
*       @global-owner1 @global-owner2

# Order is important; the last matching pattern takes the most
# precedence. When someone opens a pull request that only
# modifies JS files, only @js-owner and not the global
# owner(s) will be requested for a review.
*.js    @js-owner

# You can also use email addresses if you prefer. They'll be
# used to look up users just like we do for commit author
# emails.
*.go docs@example.com

# In this example, @doctocat owns any files in the build/logs
# directory at the root of the repository and any of its
# subdirectories.
/build/logs/ @doctocat

# The `docs/*` pattern will match files like
# `docs/getting-started.md` but not further nested files like
# `docs/build-app/troubleshooting.md`.
docs/*  docs@example.com

# In this example, @octocat owns any file in an apps directory
# anywhere in your repository.
apps/ @octocat

# In this example, @doctocat owns any file in the `/docs`
# directory in the root of your repository and any of its
# subdirectories.
/docs/ @doctocat
"#;

        let co = parse(example).unwrap();
        assert_eq!(co.rules.len(), 7);

        macro_rules! test_rule {
            ($index:expr, $pattern:expr, $owners:expr, $test:expr) => {
                let pattern = $pattern;
                let owners = $owners;
                let rule = &co.rules[$index];
                assert_eq!(rule.pattern, pattern);
                assert_eq!(rule.owners, owners);

                let m = co.matches($test);
                assert_eq!(m, Some(rule));
            };
        }

        test_rule!(
            0,
            "*",
            vec![
                Handle("@global-owner1".to_string()),
                Handle("@global-owner2".to_string()),
            ],
            "something"
        );

        test_rule!(1, "*.js", vec![Handle("@js-owner".to_string())], "index.js");
        test_rule!(
            2,
            "*.go",
            vec![Email("docs@example.com".to_string())],
            "mod.go"
        );

        test_rule!(
            3,
            "/build/logs/",
            vec![Handle("@doctocat".to_string())],
            "/build/logs/foobar"
        );

        test_rule!(
            4,
            "docs/*",
            vec![Email("docs@example.com".to_string())],
            "somewhere/docs/readme.md"
        );

        test_rule!(
            5,
            "apps/",
            vec![Handle("@octocat".to_string())],
            "anywhere/apps/test"
        );

        test_rule!(
            5,
            "apps/",
            vec![Handle("@octocat".to_string())],
            "apps/test"
        );
        test_rule!(
            6,
            "/docs/",
            vec![Handle("@doctocat".to_string())],
            "/docs/foo/bar/baz"
        );
    }

    #[test]
    fn test_parse_owner_email() {
        assert_eq!(
            Owner::parse("something@something"),
            Email("something@something".to_string())
        );
    }

    #[test]
    fn test_parse_owner_handler() {
        assert_eq!(Owner::parse("@someone"), Handle("@someone".to_string()));
        assert_eq!(
            Owner::parse("@Org/someone"),
            Handle("@Org/someone".to_string())
        );
    }

    #[test]
    fn test_parse_valid_rule() {
        assert_eq!(
            Rule::parse("some/sub/path @user   someone@example.com").unwrap(),
            Rule {
                pattern: "some/sub/path".to_owned(),
                owners: vec![
                    Owner::Handle("@user".to_owned()),
                    Owner::Email("someone@example.com".to_owned())
                ],
            }
        );
    }

    #[test]
    fn test_parse_invalid_rule() {
        assert_eq!(
            Rule::parse("some/sub/path"),
            Err(ParseError::MissingOwners {
                input: "some/sub/path".to_owned(),
            })
        );
    }
}
