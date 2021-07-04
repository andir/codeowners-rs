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

impl std::fmt::Display for Owner {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (match self {
            Self::Email(mail) => mail,
            Self::Handle(name) => name,
        }).fmt(fmt)
    }
}

/// Convert a Codeowners pattern into a glob pattern
fn pattern_to_glob(pattern: impl AsRef<str>) -> impl Iterator<Item = String> {
    let pattern = pattern.as_ref();
    let mut absolute = false;

    // patterns that start with a / only match in the root
    if pattern.starts_with('/') {
        //pattern = &pattern[1..];
        absolute = true;
    }

    let mut pattern = pattern.to_string();
    // for paths that aren't absolute but have a slash somewhere we must prefix them with **/ so
    // they match in any subdirectory.
    if !absolute && pattern.contains('/') {
        pattern = format!("**/{}", pattern);
    }

    // If a path ends with / then include all subpaths as well
    // If it doesn't well, do it nevertheless because things don't always work like in the specification
    let subdirectory_pattern = if pattern.ends_with('/') {
        format!("{}**", pattern)
    } else {
        format!("{}/**", pattern)
    };

    std::iter::once(pattern).chain(std::iter::once(subdirectory_pattern))
}

/// Representation of one Codeowner pattern and the respective list of owners.
#[derive(Debug)]
pub struct Rule {
    pub pattern: String,
    pub owners: Vec<Owner>,
    pub matchers: Vec<globset::GlobMatcher>,
}

impl PartialEq for Rule {
    fn eq(&self, other: &Rule) -> bool {
        /* We purposefully don't want to compare the glob, as it is
         * uniquely determined by the pattern
         */
        self.pattern == other.pattern && self.owners == other.owners
    }
}

impl Rule {
    pub fn parse(input: impl AsRef<str>) -> Result<Rule, ParseError> {
        let input = input.as_ref();
        // split in spaces and ignore multiple spaces
        // TODO add support for spaces in paths
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
        let matchers = pattern_to_glob(&pattern)
            .map(|pattern| globset::Glob::new(&pattern))
            .map(|glob| glob.map(|glob| glob.compile_matcher()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Rule {
          matchers,
          pattern,
          owners
        })
    }
}

/// The parsed representation of a GitHub Codeowners file
#[derive(Debug)]
pub struct Codeowners {
    pub rules: Vec<Rule>,
}

impl PartialEq for Codeowners {
    fn eq(&self, other: &Codeowners) -> bool {
        self.rules == other.rules
    }
}

impl Codeowners {
    /// Match the given path against the set of rules and return the matching rules or None.
    pub fn matches<'a, T: AsRef<std::path::Path>>(&'a self, path: T) -> Option<&'a [Owner]> {
        let path = path.as_ref();
        self.rules.iter()
            .rev()
            .find(|rule| rule.matchers.iter().any(|matcher| matcher.is_match(path)))
            .map(|rule| rule.owners.as_slice())
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

    Ok(Codeowners { rules })
}

#[cfg(test)]
mod tests {

    use super::{Owner::*, *};

    #[test]
    fn more_test_on_nixpkgs() {
        let codeowners = parse(
            std::str::from_utf8(&std::fs::read("./CODEOWNERS").unwrap()).unwrap()
        ).unwrap();
        assert!(codeowners.matches("/lib").is_some());
        assert!(codeowners.matches("/lib/systems").is_some());
        assert!(codeowners.matches("/lib/foo").is_some());
    }

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
        
        dbg!(co.rules.iter().map(|rule| &rule.pattern).enumerate().collect::<Vec<_>>());

        let test_rule = |index: usize, pattern: &str, owners: Vec<Owner>, test: &str| {
            let rule: &Rule = &co.rules[index];
            assert_eq!(pattern, rule.pattern);
            assert_eq!(owners, rule.owners);

            let matched_rule: Option<&[Owner]> = co.matches(test);
            assert_eq!(Some(rule.owners.as_slice()), matched_rule);
        };

        test_rule(
            0,
            "*",
            vec![
                Handle("@global-owner1".to_string()),
                Handle("@global-owner2".to_string()),
            ],
            "something"
        );

        test_rule(1, "*.js", vec![Handle("@js-owner".to_string())], "index.js");
        test_rule(
            2,
            "*.go",
            vec![Email("docs@example.com".to_string())],
            "mod.go"
        );

        test_rule(
            3,
            "/build/logs/",
            vec![Handle("@doctocat".to_string())],
            "/build/logs/foobar"
        );

        test_rule(
            4,
            "docs/*",
            vec![Email("docs@example.com".to_string())],
            "somewhere/docs/readme.md"
        );

        test_rule(
            5,
            "apps/",
            vec![Handle("@octocat".to_string())],
            "anywhere/apps/test"
        );

        test_rule(
            5,
            "apps/",
            vec![Handle("@octocat".to_string())],
            "apps/test"
        );
        test_rule(
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
                pattern: "some/sub/path".into(),
                matchers: vec![globset::Glob::new("some/sub/path").unwrap().compile_matcher()],
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
