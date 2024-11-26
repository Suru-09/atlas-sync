pub mod ignore_list {
    use regex::Regex;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    #[derive(Debug)]
    pub struct GitignoreRule {
        pub pattern: String,
        pub is_negated: bool,
        regex: Regex,
    }

    #[derive(Debug)]
    pub enum GitignoreError {
        InvalidPath,
        InvalidLine,
    }

    #[derive(Debug)]
    pub struct IgnoreList {
        pub ignored_list: Vec<GitignoreRule>,
    }

    impl IgnoreList {
        fn new(ignored_list: Vec<GitignoreRule>) -> Self {
            Self { ignored_list }
        }
    }

    impl GitignoreRule {
        pub fn new(pattern: String, is_negated: bool) -> Self {
            Self {
                pattern: pattern.clone(),
                is_negated,
                regex: GitignoreRule::convert_to_regex(&pattern),
            }
        }

        fn convert_to_regex(pattern: &str) -> Regex {
            let mut regex_pattern = pattern.to_string();

            regex_pattern = regex_pattern.replace(r"*", ".*");
            regex_pattern = regex_pattern.replace(r"\*", ".*");
            regex_pattern = regex_pattern.replace(r"\?", ".");
            regex_pattern = regex_pattern.replace(r"\*\*", ".*");
            regex_pattern = regex_pattern.replace(r"**", ".*");

            if pattern.ends_with('/') {
                regex_pattern.push('$');
            }
            Regex::new(&regex_pattern).unwrap()
        }

        pub fn matches(&self, haystack: &str) -> bool {
            let is_match = self.regex.is_match(haystack);
            if self.is_negated {
                !is_match
            } else {
                is_match
            }
        }
    }

    pub fn parse_gitignore(path: &Path) -> Result<IgnoreList, GitignoreError> {
        let mut rule_set: Vec<GitignoreRule> = Vec::new();
        let gitignore_file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return Err(GitignoreError::InvalidPath),
        };
        let reader = BufReader::new(gitignore_file);

        let comment_regex = Regex::new(r"^\s*(#|$)").unwrap();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => return Err(GitignoreError::InvalidLine),
            };

            // ignore empty lines/comments.
            if comment_regex.is_match(&line) {
                continue;
            }

            let mut is_negated = false;
            let mut pattern = line.trim().to_string();

            if pattern.starts_with('!') {
                is_negated = true;
                pattern = pattern[1..].to_string();
            }

            rule_set.push(GitignoreRule::new(pattern, is_negated));
        }

        rule_set.dedup_by(|r1, r2| r1.pattern == r2.pattern && r1.is_negated == r2.is_negated);

        Ok(IgnoreList::new(rule_set))
    }
}

#[cfg(test)]
mod tests {
    use ignore_list::{parse_gitignore, GitignoreRule};

    use super::*;
    use std::path::Path;

    #[test]
    fn parse_project_gitignore() {
        let gitignore_path = Path::new("../.gitignore");
        let rules = parse_gitignore(&gitignore_path).unwrap();

        let node_modules = rules
            .ignored_list
            .iter()
            .find(|&rule| rule.pattern == "node_modules");
        assert!(node_modules.is_some());

        let sveltekit = rules
            .ignored_list
            .iter()
            .find(|&rule| rule.pattern == ".sveltekit");
        assert!(sveltekit.is_some());

        let sveltekit_misspelled = rules
            .ignored_list
            .iter()
            .find(|&rule| rule.pattern == ".sveltkit");
        assert!(sveltekit_misspelled.is_none());

        let target_folder = rules
            .ignored_list
            .iter()
            .find(|&rule| rule.pattern == "target/");
        assert!(target_folder.is_some());
    }

    #[test]
    fn match_globbing() {
        let txt_rule = GitignoreRule::new(String::from("*.txt"), false);
        assert!(txt_rule.matches("haystack.txt"));

        let txt_rule = GitignoreRule::new(String::from("*/*.txt"), false);
        assert!(txt_rule.matches("wtf/test/ceva.txt"));

        let txt_rule = GitignoreRule::new(String::from("*.txt"), false);
        assert!(txt_rule.matches("/root/subroot/some_weird_text_file.txt"));
    }

    #[test]
    fn exact_match() {
        let cargo_lock_rule = GitignoreRule::new(String::from("Cargo.lock"), false);
        assert!(cargo_lock_rule.matches("Cargo.lock"));
    }
}
