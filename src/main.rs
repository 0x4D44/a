use base64::Engine;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const VERSION: &str = "1.3.0";
// Hardcoded GitHub target for config sync
const GITHUB_REPO: &str = "0x4d44/a"; // owner/repo
const GITHUB_BRANCH: &str = "main";
const GITHUB_CONFIG_PATH: &str = "config.json";

// ANSI color codes
const COLOR_RESET: &str = "\x1b[0m";
const COLOR_BOLD: &str = "\x1b[1m";
const COLOR_GREEN: &str = "\x1b[32m";
const COLOR_BLUE: &str = "\x1b[34m";
const COLOR_CYAN: &str = "\x1b[36m";
const COLOR_YELLOW: &str = "\x1b[33m";
const COLOR_GRAY: &str = "\x1b[90m";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum ChainOperator {
    And,         // && - run if previous succeeded
    Or,          // || - run if previous failed
    Always,      // ; - always run regardless
    IfCode(i32), // run if previous exit code equals N
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChainCommand {
    command: String,
    operator: Option<ChainOperator>, // None for the first command
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CommandChain {
    commands: Vec<ChainCommand>,
    parallel: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum CommandType {
    Simple(String),      // Single command (backward compatibility)
    Chain(CommandChain), // Complex command chain
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AliasEntry {
    command_type: CommandType,
    description: Option<String>,
    created: String,
}

impl AliasEntry {
    // Helper method to get command string for display (backward compatibility)
    fn command_display(&self) -> String {
        match &self.command_type {
            CommandType::Simple(cmd) => cmd.clone(),
            CommandType::Chain(chain) => {
                let mut result = String::new();
                for (i, chain_cmd) in chain.commands.iter().enumerate() {
                    if i > 0 {
                        let op_str = match &chain_cmd.operator {
                            Some(ChainOperator::And) => " && ",
                            Some(ChainOperator::Or) => " || ",
                            Some(ChainOperator::Always) => " ; ",
                            Some(ChainOperator::IfCode(code)) => &format!(" ?[{}] ", code),
                            None => " ",
                        };
                        result.push_str(op_str);
                    }
                    result.push_str(&chain_cmd.command);
                }
                if chain.parallel {
                    format!("PARALLEL: {}", result)
                } else {
                    result
                }
            }
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Config {
    aliases: HashMap<String, AliasEntry>,
}

impl Config {
    fn new() -> Self {
        Config {
            aliases: HashMap::new(),
        }
    }

    fn add_alias(
        &mut self,
        name: String,
        command_type: CommandType,
        description: Option<String>,
        force: bool,
    ) -> Result<bool, String> {
        if name.starts_with("--") || name.contains("mgr:") || name.starts_with(".") {
            return Err(format!(
                "Invalid alias name '{}': cannot use reserved prefixes",
                name
            ));
        }

        let is_overwrite = self.aliases.contains_key(&name);
        if is_overwrite && !force {
            return Ok(false); // Signal that confirmation is needed
        }

        let entry = AliasEntry {
            command_type,
            description,
            created: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        };

        self.aliases.insert(name, entry);
        Ok(true) // Successfully added/updated
    }

    fn remove_alias(&mut self, name: &str) -> Result<(), String> {
        if self.aliases.remove(name).is_some() {
            Ok(())
        } else {
            Err(format!("Alias '{}' not found", name))
        }
    }

    fn get_alias(&self, name: &str) -> Option<&AliasEntry> {
        self.aliases.get(name)
    }

    fn list_aliases(&self, filter: Option<&str>) -> Vec<(&String, &AliasEntry)> {
        let mut aliases: Vec<_> = self.aliases.iter().collect();

        if let Some(pattern) = filter {
            aliases.retain(|(name, _)| name.contains(pattern));
        }

        aliases.sort_by_key(|(name, _)| *name);
        aliases
    }
}

struct AliasManager {
    config: Config,
    config_path: PathBuf,
}

impl AliasManager {
    fn new() -> Result<Self, String> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_config(&config_path)?;

        Ok(AliasManager {
            config,
            config_path,
        })
    }

    fn get_config_path() -> Result<PathBuf, String> {
        let home_dir = if cfg!(windows) {
            env::var("USERPROFILE").map_err(|_| "USERPROFILE environment variable not found")?
        } else {
            env::var("HOME").map_err(|_| "HOME environment variable not found")?
        };

        let mut config_dir = PathBuf::from(home_dir);
        config_dir.push(".alias-mgr");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        config_dir.push("config.json");
        Ok(config_dir)
    }

    fn load_config(path: &PathBuf) -> Result<Config, String> {
        if !path.exists() {
            return Ok(Config::new());
        }

        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;

        // Try to parse as new format first
        match serde_json::from_str::<Config>(&content) {
            Ok(config) => Ok(config),
            Err(_) => {
                // Try to parse as legacy format and migrate
                Self::migrate_legacy_config(&content)
            }
        }
    }

    fn migrate_legacy_config(content: &str) -> Result<Config, String> {
        // Legacy format has "command" field instead of "command_type"
        #[derive(serde::Deserialize)]
        struct LegacyAliasEntry {
            command: String,
            description: Option<String>,
            created: String,
        }

        #[derive(serde::Deserialize)]
        struct LegacyConfig {
            aliases: HashMap<String, LegacyAliasEntry>,
        }

        let legacy_config: LegacyConfig = serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse legacy config file: {}", e))?;

        // Convert to new format
        let mut new_config = Config::new();
        for (name, legacy_entry) in legacy_config.aliases {
            let command_type = if legacy_entry.command.contains(" && ") {
                // Convert legacy chained commands to simple format for now
                CommandType::Simple(legacy_entry.command)
            } else {
                CommandType::Simple(legacy_entry.command)
            };

            let new_entry = AliasEntry {
                command_type,
                description: legacy_entry.description,
                created: legacy_entry.created,
            };

            new_config.aliases.insert(name, new_entry);
        }

        Ok(new_config)
    }

    fn save_config(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&self.config_path, content)
            .map_err(|e| format!("Failed to save config file: {}", e))
    }

    fn github_token() -> Option<String> {
        env::var("A_GITHUB_TOKEN")
            .ok()
            .or_else(|| env::var("GITHUB_TOKEN").ok())
            .or_else(|| env::var("GH_TOKEN").ok())
    }

    fn push_config_to_github(&self, message: Option<&str>) -> Result<(), String> {
        let repo = GITHUB_REPO;
        let branch = GITHUB_BRANCH;
        let path_in_repo = GITHUB_CONFIG_PATH;
        let commit_message = message.unwrap_or("chore(config): update alias config");

        let token = Self::github_token().ok_or_else(|| {
            "Missing GitHub token. Set A_GITHUB_TOKEN or GITHUB_TOKEN.".to_string()
        })?;

        if !self.config_path.exists() {
            return Err(
                "Source config file does not exist. Create some aliases first.".to_string(),
            );
        }

        let content_bytes = fs::read(&self.config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let content_b64 = base64::engine::general_purpose::STANDARD.encode(content_bytes);

        let api_base = format!(
            "https://api.github.com/repos/{}/contents/{}",
            repo, path_in_repo
        );
        let get_url = format!("{}?ref={}", api_base, branch);

        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();

        let mut maybe_sha: Option<String> = None;
        let get_req = agent
            .get(&get_url)
            .set("User-Agent", "a-alias-manager")
            .set("Authorization", &format!("Bearer {}", token));

        match get_req.call() {
            Ok(resp) => {
                if resp.status() == 200 {
                    if let Ok(val) = resp.into_json::<serde_json::Value>() {
                        if let Some(sha) = val.get("sha").and_then(|v| v.as_str()) {
                            maybe_sha = Some(sha.to_string());
                        }
                    }
                }
            }
            Err(ureq::Error::Status(404, _)) => {}
            Err(e) => {
                return Err(format!("Failed to query existing file: {}", e));
            }
        }

        let mut body = serde_json::json!({
            "message": commit_message,
            "content": content_b64,
            "branch": branch,
        });
        if let Some(sha) = maybe_sha {
            body["sha"] = serde_json::Value::String(sha);
        }

        let put_req = agent
            .put(&api_base)
            .set("User-Agent", "a-alias-manager")
            .set("Authorization", &format!("Bearer {}", token))
            .send_json(body);

        match put_req {
            Ok(resp) => {
                if resp.status() == 200 || resp.status() == 201 {
                    println!(
                        "{}Config pushed to GitHub:{} https://github.com/{}/blob/{}/{}",
                        COLOR_GREEN, COLOR_RESET, repo, branch, path_in_repo
                    );
                    Ok(())
                } else {
                    Err(format!("GitHub API returned status {}", resp.status()))
                }
            }
            Err(e) => Err(format!("Failed to push to GitHub: {}", e)),
        }
    }

    fn pull_config_from_github(&mut self) -> Result<(), String> {
        let repo = GITHUB_REPO;
        let branch = GITHUB_BRANCH;
        let path_in_repo = GITHUB_CONFIG_PATH;

        let token_opt = Self::github_token();

        let api_url = format!(
            "https://api.github.com/repos/{}/contents/{}?ref={}",
            repo, path_in_repo, branch
        );
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        let mut req = agent.get(&api_url).set("User-Agent", "a-alias-manager");
        if let Some(token) = &token_opt {
            req = req.set("Authorization", &format!("Bearer {}", token));
        }

        let resp = req
            .call()
            .map_err(|e| format!("Failed to fetch config: {}", e))?;
        if resp.status() != 200 {
            return Err(format!("GitHub API returned status {}", resp.status()));
        }

        let val: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

        let encoding = val
            .get("encoding")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing encoding in GitHub response".to_string())?;
        if encoding != "base64" {
            return Err("Unsupported encoding from GitHub".to_string());
        }
        let content_b64 = val
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing content in GitHub response".to_string())?;

        let content_clean = content_b64.replace('\n', "");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content_clean)
            .map_err(|e| format!("Failed to decode content: {}", e))?;
        let text = String::from_utf8(bytes).map_err(|e| format!("Invalid UTF-8 content: {}", e))?;

        let parsed: Config = serde_json::from_str(&text)
            .map_err(|e| format!("Downloaded config is invalid JSON: {}", e))?;

        if self.config_path.exists() {
            let mut backup_path = self.config_path.clone();
            backup_path.set_file_name("config.backup.json");
            fs::copy(&self.config_path, &backup_path)
                .map_err(|e| format!("Failed to create backup: {}", e))?;
            println!(
                "{}Existing config backed up to:{} {}",
                COLOR_GRAY,
                COLOR_RESET,
                backup_path.display()
            );
        }

        fs::write(&self.config_path, text)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        self.config = parsed;

        println!(
            "{}Config pulled from GitHub:{} https://github.com/{}/blob/{}/{}",
            COLOR_GREEN, COLOR_RESET, repo, branch, path_in_repo
        );
        println!(
            "{}File contains {} aliases{}",
            COLOR_GRAY,
            self.config.aliases.len(),
            COLOR_RESET
        );

        Ok(())
    }

    fn add_alias(
        &mut self,
        name: String,
        command_type: CommandType,
        description: Option<String>,
        force: bool,
    ) -> Result<(), String> {
        // Check if alias already exists before making changes
        let alias_existed = self.config.aliases.contains_key(&name);

        // Check if alias exists and get confirmation if needed
        let confirmed_force = if alias_existed && !force {
            let existing = self.config.get_alias(&name).unwrap();
            println!(
                "{}Alias '{}' already exists:{}",
                COLOR_YELLOW, name, COLOR_RESET
            );
            println!(
                "  {}Current:{} {}",
                COLOR_CYAN,
                COLOR_RESET,
                existing.command_display()
            );
            if let Some(desc) = &existing.description {
                println!("  {}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }
            println!(
                "  {}New:{} {}",
                COLOR_CYAN,
                COLOR_RESET,
                match &command_type {
                    CommandType::Simple(cmd) => cmd.clone(),
                    CommandType::Chain(chain) =>
                        format!("Complex chain with {} commands", chain.commands.len()),
                }
            );

            if !Self::confirm_overwrite()? {
                println!("{}Alias not modified.{}", COLOR_GRAY, COLOR_RESET);
                return Ok(());
            }
            true // User confirmed, so force the update
        } else {
            force // Use the original force value
        };

        match self
            .config
            .add_alias(name.clone(), command_type, description, confirmed_force)
        {
            Ok(true) => {
                self.save_config()?;
                if alias_existed {
                    println!("{}Updated alias '{}'{}", COLOR_GREEN, name, COLOR_RESET);
                } else {
                    println!("{}Added alias '{}'{}", COLOR_GREEN, name, COLOR_RESET);
                }
                Ok(())
            }
            Ok(false) => {
                // This shouldn't happen with the current logic, but handle it gracefully
                Err("Unexpected confirmation state".to_string())
            }
            Err(e) => Err(e),
        }
    }

    fn confirm_overwrite() -> Result<bool, String> {
        print!("{}Overwrite? (y/N):{} ", COLOR_YELLOW, COLOR_RESET);
        io::stdout()
            .flush()
            .map_err(|e| format!("Failed to flush stdout: {}", e))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("Failed to read input: {}", e))?;

        let response = input.trim().to_lowercase();
        Ok(response == "y" || response == "yes")
    }

    fn remove_alias(&mut self, name: &str) -> Result<(), String> {
        self.config.remove_alias(name)?;
        self.save_config()
    }

    fn list_aliases(&self, filter: Option<&str>) {
        let aliases = self.config.list_aliases(filter);

        if aliases.is_empty() {
            if filter.is_some() {
                println!(
                    "{}No aliases found matching filter.{}",
                    COLOR_YELLOW, COLOR_RESET
                );
            } else {
                println!("{}No aliases configured.{}", COLOR_YELLOW, COLOR_RESET);
            }
            return;
        }

        println!(
            "{}{}Configured aliases:{}",
            COLOR_BOLD, COLOR_CYAN, COLOR_RESET
        );

        // Calculate the maximum alias name length for alignment
        let max_name_len = aliases
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0);
        let name_width = std::cmp::max(16, ((max_name_len + 4) / 4) * 4); // Minimum 16 chars, rounded to 4

        for (name, entry) in aliases {
            let padding = name_width.saturating_sub(name.len());
            let spaces = " ".repeat(padding);

            print!(
                "  {}{}{}{} -> {}{}{}",
                COLOR_GREEN,
                name,
                COLOR_RESET,
                spaces,
                COLOR_BLUE,
                entry.command_display(),
                COLOR_RESET
            );

            if let Some(desc) = &entry.description {
                print!(" {}({}){}", COLOR_GRAY, desc, COLOR_RESET);
            }

            println!(" {}[{}]{}", COLOR_GRAY, entry.created, COLOR_RESET);
        }
    }

    fn which_alias(&self, name: &str) {
        if let Some(entry) = self.config.get_alias(name) {
            println!(
                "{}Alias '{}' executes:{} {}",
                COLOR_CYAN,
                name,
                COLOR_RESET,
                entry.command_display()
            );
            if let Some(desc) = &entry.description {
                println!("{}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }

            // Check if any commands contain parameter variables
            let has_variables = match &entry.command_type {
                CommandType::Simple(cmd) => Self::has_parameter_variables(cmd),
                CommandType::Chain(chain) => chain
                    .commands
                    .iter()
                    .any(|cmd| Self::has_parameter_variables(&cmd.command)),
            };

            // Show parameter substitution examples if variables are present
            if has_variables {
                println!(
                    "{}Parameter substitution example:{}",
                    COLOR_CYAN, COLOR_RESET
                );
                let example_args = vec!["arg1".to_string(), "arg2".to_string(), "arg3".to_string()];

                match &entry.command_type {
                    CommandType::Simple(cmd) => {
                        let resolved = Self::substitute_parameters(cmd, &example_args);
                        println!(
                            "  {}a{} {} {}arg1 arg2 arg3{}",
                            COLOR_GREEN, COLOR_RESET, name, COLOR_YELLOW, COLOR_RESET
                        );
                        println!("  {}Resolves to:{} {}", COLOR_GRAY, COLOR_RESET, resolved);
                    }
                    CommandType::Chain(chain) => {
                        println!(
                            "  {}a{} {} {}arg1 arg2 arg3{}",
                            COLOR_GREEN, COLOR_RESET, name, COLOR_YELLOW, COLOR_RESET
                        );
                        println!("  {}Resolves to:{}", COLOR_GRAY, COLOR_RESET);
                        for (i, chain_cmd) in chain.commands.iter().enumerate() {
                            let resolved =
                                Self::substitute_parameters(&chain_cmd.command, &example_args);
                            let op_prefix = if i > 0 { " && " } else { "" };
                            println!("    {}{}{}", COLOR_BLUE, op_prefix, resolved);
                        }
                    }
                }
                println!();
            }

            // Show detailed breakdown for complex chains
            if let CommandType::Chain(chain) = &entry.command_type {
                println!("{}Command breakdown:{}", COLOR_CYAN, COLOR_RESET);
                for (i, chain_cmd) in chain.commands.iter().enumerate() {
                    let op_desc = match &chain_cmd.operator {
                        Some(ChainOperator::And) => " (run if previous succeeded)",
                        Some(ChainOperator::Or) => " (run if previous failed)",
                        Some(ChainOperator::Always) => " (always run)",
                        Some(ChainOperator::IfCode(code)) => {
                            &format!(" (run if previous exit code = {})", code)
                        }
                        None => "",
                    };
                    let has_vars = if Self::has_parameter_variables(&chain_cmd.command) {
                        " 📋"
                    } else {
                        ""
                    };
                    println!(
                        "  {}{}. {}{}{}{}{}",
                        COLOR_GRAY,
                        i + 1,
                        COLOR_RESET,
                        chain_cmd.command,
                        has_vars,
                        COLOR_GRAY,
                        op_desc
                    );
                }
                if chain.parallel {
                    println!("{}Execution mode:{} Parallel", COLOR_CYAN, COLOR_RESET);
                } else {
                    println!("{}Execution mode:{} Sequential", COLOR_CYAN, COLOR_RESET);
                }
            }
        } else {
            println!("{}Alias '{}' not found.{}", COLOR_YELLOW, name, COLOR_RESET);
        }
    }

    fn show_config_location(&self) {
        println!(
            "{}Config file location:{} {}",
            COLOR_CYAN,
            COLOR_RESET,
            self.config_path.display()
        );
    }

    fn export_config(&self, target_path: Option<&str>) -> Result<(), String> {
        // Determine target directory - current directory if not specified
        let target_dir = if let Some(path) = target_path {
            PathBuf::from(path)
        } else {
            env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?
        };

        // Ensure target is a directory (or create it if it doesn't exist)
        if target_dir.exists() && !target_dir.is_dir() {
            return Err(format!(
                "Target path '{}' exists but is not a directory",
                target_dir.display()
            ));
        }

        if !target_dir.exists() {
            fs::create_dir_all(&target_dir).map_err(|e| {
                format!(
                    "Failed to create target directory '{}': {}",
                    target_dir.display(),
                    e
                )
            })?;
        }

        // Construct target file path
        let target_file = target_dir.join("config.json");

        // Check if source config file exists
        if !self.config_path.exists() {
            return Err(
                "Source config file does not exist. Create some aliases first.".to_string(),
            );
        }

        // Copy the config file
        fs::copy(&self.config_path, &target_file)
            .map_err(|e| format!("Failed to copy config file: {}", e))?;

        println!(
            "{}Config exported to:{} {}",
            COLOR_GREEN,
            COLOR_RESET,
            target_file.display()
        );
        println!(
            "{}File contains {} aliases{}",
            COLOR_GRAY,
            self.config.aliases.len(),
            COLOR_RESET
        );

        Ok(())
    }

    fn execute_alias(&self, name: &str, args: &[String]) -> Result<(), String> {
        let entry = self
            .config
            .get_alias(name)
            .ok_or_else(|| format!("Alias '{}' not found", name))?;

        match &entry.command_type {
            CommandType::Simple(command) => {
                // Check if this is a legacy chained command (contains &&)
                if command.contains(" && ") {
                    self.execute_legacy_command_chain(command, args)
                } else {
                    self.execute_single_command(command, args)
                }
            }
            CommandType::Chain(chain) => {
                if chain.parallel {
                    self.execute_parallel_chain(chain, args)
                } else {
                    self.execute_sequential_chain(chain, args)
                }
            }
        }
    }

    fn execute_legacy_command_chain(
        &self,
        full_command: &str,
        additional_args: &[String],
    ) -> Result<(), String> {
        let commands: Vec<&str> = full_command.split(" && ").collect();

        for (index, command_str) in commands.iter().enumerate() {
            let command_str = command_str.trim();
            if command_str.is_empty() {
                continue;
            }

            // Only add additional args to the last command in the chain
            let args_to_use = if index == commands.len() - 1 {
                additional_args
            } else {
                &[]
            };

            println!(
                "{}[{}/{}]{} Executing: {}{}{}",
                COLOR_GRAY,
                index + 1,
                commands.len(),
                COLOR_RESET,
                COLOR_CYAN,
                command_str,
                COLOR_RESET
            );

            match self.execute_single_command(command_str, args_to_use) {
                Ok(()) => continue,
                Err(e) => {
                    eprintln!("{}Command failed:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    eprintln!(
                        "{}Stopping command chain at step {}/{}{}",
                        COLOR_YELLOW,
                        index + 1,
                        commands.len(),
                        COLOR_RESET
                    );
                    return Err(format!("Command chain stopped at step {}", index + 1));
                }
            }
        }

        println!(
            "{}Command chain completed successfully{}",
            COLOR_GREEN, COLOR_RESET
        );
        Ok(())
    }

    fn execute_sequential_chain(
        &self,
        chain: &CommandChain,
        additional_args: &[String],
    ) -> Result<(), String> {
        let mut last_exit_code = 0;

        for (index, chain_cmd) in chain.commands.iter().enumerate() {
            let should_execute = match &chain_cmd.operator {
                None => true, // First command always executes
                Some(ChainOperator::And) => last_exit_code == 0,
                Some(ChainOperator::Or) => last_exit_code != 0,
                Some(ChainOperator::Always) => true,
                Some(ChainOperator::IfCode(code)) => last_exit_code == *code,
            };

            if !should_execute {
                let reason = match &chain_cmd.operator {
                    Some(ChainOperator::And) => {
                        format!("previous command failed (exit code {})", last_exit_code)
                    }
                    Some(ChainOperator::Or) => "previous command succeeded".to_string(),
                    Some(ChainOperator::IfCode(code)) => format!(
                        "previous exit code was {}, expected {}",
                        last_exit_code, code
                    ),
                    _ => "unknown condition".to_string(),
                };
                println!(
                    "{}[{}/{}]{} Skipping: {}{}{} ({})",
                    COLOR_GRAY,
                    index + 1,
                    chain.commands.len(),
                    COLOR_RESET,
                    COLOR_GRAY,
                    chain_cmd.command,
                    COLOR_RESET,
                    reason
                );
                continue;
            }

            // If any command in the chain has parameter variables, pass args to all commands
            // Otherwise, only pass args to the last command (backward compatibility)
            let has_vars_in_chain = chain
                .commands
                .iter()
                .any(|cmd| Self::has_parameter_variables(&cmd.command));
            let args_to_use = if has_vars_in_chain || index == chain.commands.len() - 1 {
                additional_args
            } else {
                &[]
            };

            let op_desc = match &chain_cmd.operator {
                Some(ChainOperator::And) => " (&&)",
                Some(ChainOperator::Or) => " (||)",
                Some(ChainOperator::Always) => " (;)",
                Some(ChainOperator::IfCode(code)) => &format!(" (?[{}])", code),
                None => "",
            };

            println!(
                "{}[{}/{}]{}{} Executing: {}{}{}",
                COLOR_GRAY,
                index + 1,
                chain.commands.len(),
                COLOR_RESET,
                op_desc,
                COLOR_CYAN,
                chain_cmd.command,
                COLOR_RESET
            );

            last_exit_code = self
                .execute_single_command_with_exit_code(&chain_cmd.command, args_to_use)
                .unwrap_or({
                    // Command failed to execute (e.g., program not found)
                    // Treat this as exit code 127 (command not found) and continue
                    127
                });
        }

        println!(
            "{}Sequential command chain completed{}",
            COLOR_GREEN, COLOR_RESET
        );
        Ok(())
    }

    fn execute_parallel_chain(
        &self,
        chain: &CommandChain,
        additional_args: &[String],
    ) -> Result<(), String> {
        use std::sync::mpsc;
        use std::thread;

        println!(
            "{}Executing {} commands in parallel{}",
            COLOR_CYAN,
            chain.commands.len(),
            COLOR_RESET
        );

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();

        for (index, chain_cmd) in chain.commands.iter().enumerate() {
            let cmd = chain_cmd.command.clone();
            let cmd_display = cmd.clone(); // Clone for display purposes
                                           // If any command in the chain has parameter variables, pass args to all commands
                                           // Otherwise, only pass args to the last command (backward compatibility)
            let has_vars_in_chain = chain
                .commands
                .iter()
                .any(|cmd| Self::has_parameter_variables(&cmd.command));
            let args = if has_vars_in_chain || index == chain.commands.len() - 1 {
                additional_args.to_vec()
            } else {
                Vec::new()
            };
            let tx = tx.clone();

            let handle = thread::spawn(move || {
                let result = Self::execute_command_static(&cmd, &args);
                tx.send((index, result)).unwrap();
            });

            handles.push(handle);
            println!(
                "{}Started:{} {}{}{}",
                COLOR_GRAY, COLOR_RESET, COLOR_CYAN, cmd_display, COLOR_RESET
            );
        }

        drop(tx); // Close the sender

        let mut results = Vec::new();
        for _ in 0..chain.commands.len() {
            match rx.recv() {
                Ok((index, result)) => {
                    let success = result.is_ok();
                    results.push((index, result));
                    if success {
                        let code = results.last().unwrap().1.as_ref().unwrap();
                        println!(
                            "{}Completed [{}]:{} exit code {}",
                            COLOR_GREEN,
                            index + 1,
                            COLOR_RESET,
                            code
                        );
                    } else {
                        let error = results.last().unwrap().1.as_ref().err().unwrap();
                        println!(
                            "{}Failed [{}]:{} {}",
                            COLOR_YELLOW,
                            index + 1,
                            COLOR_RESET,
                            error
                        );
                    }
                }
                Err(_) => return Err("Failed to receive command results".to_string()),
            }
        }

        // Wait for all threads to finish
        for handle in handles {
            handle.join().map_err(|_| "Thread panicked")?;
        }

        // Check if any commands failed
        let failed_commands: Vec<_> = results
            .iter()
            .filter(|(_, result)| result.is_err())
            .collect();

        if failed_commands.is_empty() {
            println!(
                "{}All parallel commands completed successfully{}",
                COLOR_GREEN, COLOR_RESET
            );
            Ok(())
        } else {
            eprintln!(
                "{}Failed commands: {}/{}{}",
                COLOR_YELLOW,
                failed_commands.len(),
                chain.commands.len(),
                COLOR_RESET
            );
            Err(format!(
                "{} parallel commands failed",
                failed_commands.len()
            ))
        }
    }

    fn execute_single_command_with_exit_code(
        &self,
        command_str: &str,
        args: &[String],
    ) -> Result<i32, String> {
        // Apply parameter substitution if the command contains variables
        let resolved_command = if Self::has_parameter_variables(command_str) {
            Self::substitute_parameters(command_str, args)
        } else {
            command_str.to_string()
        };

        let mut command_parts: Vec<&str> = resolved_command.split_whitespace().collect();

        if command_parts.is_empty() {
            return Err("Empty command in alias".to_string());
        }

        let program = command_parts.remove(0);

        // For backward compatibility: if no parameter variables were found,
        // append args to maintain existing behavior
        if !Self::has_parameter_variables(command_str) {
            command_parts.extend(args.iter().map(|s| s.as_str()));
        }

        let mut cmd = Command::new(program);
        cmd.args(&command_parts);

        // Inherit stdio so the command runs interactively
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to execute command '{}': {}", program, e))?;

        Ok(status.code().unwrap_or(1))
    }

    fn execute_command_static(command_str: &str, args: &[String]) -> Result<i32, String> {
        // Apply parameter substitution if the command contains variables
        let resolved_command = if Self::has_parameter_variables(command_str) {
            Self::substitute_parameters(command_str, args)
        } else {
            command_str.to_string()
        };

        let mut command_parts: Vec<&str> = resolved_command.split_whitespace().collect();

        if command_parts.is_empty() {
            return Err("Empty command in alias".to_string());
        }

        let program = command_parts.remove(0);

        // For backward compatibility: if no parameter variables were found,
        // append args to maintain existing behavior
        if !Self::has_parameter_variables(command_str) {
            command_parts.extend(args.iter().map(|s| s.as_str()));
        }

        let mut cmd = Command::new(program);
        cmd.args(&command_parts);

        // Inherit stdio so the command runs interactively
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to execute command '{}': {}", program, e))?;

        Ok(status.code().unwrap_or(1))
    }

    fn execute_single_command(&self, command_str: &str, args: &[String]) -> Result<(), String> {
        // Apply parameter substitution if the command contains variables
        let resolved_command = if Self::has_parameter_variables(command_str) {
            Self::substitute_parameters(command_str, args)
        } else {
            command_str.to_string()
        };

        let mut command_parts: Vec<&str> = resolved_command.split_whitespace().collect();

        if command_parts.is_empty() {
            return Err("Empty command in alias".to_string());
        }

        let program = command_parts.remove(0);

        // For backward compatibility: if no parameter variables were found,
        // append args to maintain existing behavior
        if !Self::has_parameter_variables(command_str) {
            command_parts.extend(args.iter().map(|s| s.as_str()));
        }

        let mut cmd = Command::new(program);
        cmd.args(&command_parts);

        // Inherit stdio so the command runs interactively
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to execute command '{}': {}", program, e))?;

        if !status.success() {
            let exit_code = status.code().unwrap_or(1);
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }

        Ok(())
    }
    fn substitute_parameters(command: &str, args: &[String]) -> String {
        let mut result = String::new();
        let mut chars = command.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                if let Some(&next_ch) = chars.peek() {
                    match next_ch {
                        '$' => {
                            // $$ -> literal $
                            chars.next(); // consume the second $
                            result.push('$');
                        }
                        '@' => {
                            // $@ -> all arguments as separate parameters (space-separated)
                            chars.next(); // consume the @
                            result.push_str(&args.join(" "));
                        }
                        '*' => {
                            // $* -> all arguments as single string (space-separated)
                            chars.next(); // consume the *
                            result.push_str(&args.join(" "));
                        }
                        '0'..='9' => {
                            // $N -> Nth argument (1-indexed)
                            let digit_char = chars.next().unwrap();
                            if let Some(digit) = digit_char.to_digit(10) {
                                let index = digit as usize;
                                if index > 0 && index <= args.len() {
                                    result.push_str(&args[index - 1]);
                                }
                                // If index is out of bounds, substitute with empty string
                            }
                        }
                        _ => {
                            // $ followed by non-special character, treat as literal
                            result.push(ch);
                        }
                    }
                } else {
                    // $ at end of string, treat as literal
                    result.push(ch);
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    fn has_parameter_variables(command: &str) -> bool {
        let mut chars = command.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                if let Some(&next_ch) = chars.peek() {
                    match next_ch {
                        '$' => {
                            chars.next(); // consume the second $
                        }
                        '@' | '*' => {
                            return true;
                        }
                        '0'..='9' => {
                            return true;
                        }
                        _ => {}
                    }
                }
            }
        }

        false
    }
}

fn print_help() {
    // Main help content
    println!(
        "{}{}🚀 Alias Manager v{} - Cross-platform command alias tool{}",
        COLOR_BOLD, COLOR_CYAN, VERSION, COLOR_RESET
    );
    println!();

    println!("{}📋 USAGE:{}", COLOR_BOLD, COLOR_RESET);
    println!(
        "  {}a{} {}[alias_name] [args...]{}     Execute an alias",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--add <n> <command> [OPTIONS]{}",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--list [filter]{}            List aliases (optionally filtered)",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--remove <n>{}               Remove an alias",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--which <n>{}                Show what an alias does",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--config{}                   Show config file location",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--export [dir]{}             Export config to directory (default: current)",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--push{}                     Push config to GitHub (repo fixed)",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--pull{}                     Pull config from GitHub (repo fixed)",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--version{}                  Show version information",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} {}--help{}                     Show this help",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!();

    println!("{}⚙️  ADD OPTIONS:{}", COLOR_BOLD, COLOR_RESET);
    println!(
        "  {}--desc{} {}\"description\"{}        Add a description",
        COLOR_YELLOW, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!(
        "  {}--force{}                      Overwrite existing alias without confirmation",
        COLOR_YELLOW, COLOR_RESET
    );
    println!(
        "  {}--chain{} {}<command>{}            Legacy: Chain with && (same as --and)",
        COLOR_YELLOW, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!();

    println!("{}🔗 CHAINING OPERATORS:{}", COLOR_BOLD, COLOR_RESET);
    println!(
        "  {}--and{} {}<command>{}              Chain command (run if previous succeeded)",
        COLOR_GREEN, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!(
        "  {}--or{} {}<command>{}               Chain command (run if previous failed)",
        COLOR_YELLOW, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!(
        "  {}--always{} {}<command>{}           Chain command (always run regardless)",
        COLOR_BLUE, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!(
        "  {}--if-code{} {}<N> <command>{}      Chain command (run if previous exit code = N)",
        COLOR_CYAN, COLOR_RESET, COLOR_GRAY, COLOR_RESET
    );
    println!(
        "  {}--parallel{}                   Execute all commands in parallel",
        COLOR_CYAN, COLOR_RESET
    );
    println!();

    println!("{}📋 PARAMETER SUBSTITUTION:{}", COLOR_BOLD, COLOR_RESET);
    println!(
        "  {}$1, $2, $3...{}               Substitute with 1st, 2nd, 3rd argument",
        COLOR_GREEN, COLOR_RESET
    );
    println!(
        "  {}$@{}                          Substitute with all arguments",
        COLOR_GREEN, COLOR_RESET
    );
    println!(
        "  {}$*{}                          Substitute with all arguments",
        COLOR_GREEN, COLOR_RESET
    );
    println!(
        "  {}$${}                          Literal dollar sign",
        COLOR_GREEN, COLOR_RESET
    );
    println!();

    // Ask if user wants to see examples
    if should_show_examples() {
        print_examples();
    } else {
        println!(
            "{}💡 Tip:{} Use {}a --help{} and press {}Enter{} to see detailed examples",
            COLOR_CYAN, COLOR_RESET, COLOR_GREEN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET
        );
    }
}

fn should_show_examples() -> bool {
    print!(
        "{}📚 Show detailed examples? (Y/n):{} ",
        COLOR_CYAN, COLOR_RESET
    );
    io::stdout().flush().unwrap_or(());

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {
            let response = input.trim().to_lowercase();
            response.is_empty() || response == "y" || response == "yes"
        }
        Err(_) => true, // Default to showing examples if input fails
    }
}

fn print_examples() {
    println!();
    println!("{}📖 EXAMPLES:{}", COLOR_BOLD, COLOR_RESET);
    println!();

    println!("  {}# Simple alias{}", COLOR_GRAY, COLOR_RESET);
    println!(
        "  {}a --add{} gst {}\"git status\"{} {}--desc{} {}\"Quick git status\"{}",
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET,
        COLOR_YELLOW,
        COLOR_RESET,
        COLOR_GRAY,
        COLOR_RESET
    );
    println!();

    println!(
        "  {}# Sequential execution (default){}",
        COLOR_GRAY, COLOR_RESET
    );
    println!("  {}a --add{} deploy {}\"npm run build\"{} {}--and{} {}\"npm test\"{} {}--and{} {}\"npm run deploy\"{}", 
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET);
    println!();

    println!("  {}# Complex conditional logic{}", COLOR_GRAY, COLOR_RESET);
    println!("  {}a --add{} smart {}\"npm test\"{} {}--and{} {}\"npm run deploy\"{} {}--or{} {}\"echo 'Tests failed!'\"{}", 
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_YELLOW, COLOR_RESET, COLOR_BLUE, COLOR_RESET);
    println!();

    println!("  {}# Exit code handling{}", COLOR_GRAY, COLOR_RESET);
    println!("  {}a --add{} check {}\"npm test\"{} {}--if-code{} {}0{} {}\"echo 'All good!'\"{} {}--if-code{} {}1{} {}\"echo 'Tests failed'\"{}", 
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_CYAN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_CYAN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET, COLOR_BLUE, COLOR_RESET);
    println!();

    println!("  {}# Parallel execution{}", COLOR_GRAY, COLOR_RESET);
    println!(
        "  {}a --add{} build {}\"npm run lint\"{} {}--and{} {}\"npm run test\"{} {}--parallel{}",
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET,
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET,
        COLOR_CYAN,
        COLOR_RESET
    );
    println!();

    println!("  {}# Always run cleanup{}", COLOR_GRAY, COLOR_RESET);
    println!("  {}a --add{} deploy {}\"npm run build\"{} {}--and{} {}\"npm run deploy\"{} {}--always{} {}\"npm run cleanup\"{}", 
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET,
             COLOR_BLUE, COLOR_RESET, COLOR_BLUE, COLOR_RESET);
    println!();

    println!("  {}# Parameter substitution{}", COLOR_GRAY, COLOR_RESET);
    println!(
        "  {}a --add{} tag-push {}\"git tag $1\"{} {}--and{} {}\"git push origin $1\"{}",
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET,
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET
    );
    println!("  {}a{} tag-push {}v1.2.3{}               # Runs: git tag v1.2.3 && git push origin v1.2.3", 
             COLOR_GREEN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET);
    println!();

    println!("  {}# Multiple parameters{}", COLOR_GRAY, COLOR_RESET);
    println!(
        "  {}a --add{} deploy {}\"docker tag $1:$2\"{} {}--and{} {}\"docker push $1:$2\"{}",
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET,
        COLOR_GREEN,
        COLOR_RESET,
        COLOR_BLUE,
        COLOR_RESET
    );
    println!("  {}a{} deploy {}myapp latest{}           # Runs: docker tag myapp:latest && docker push myapp:latest", 
             COLOR_GREEN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET);
    println!();

    println!("  {}# All arguments with $@{}", COLOR_GRAY, COLOR_RESET);
    println!(
        "  {}a --add{} test-files {}\"pytest $@\"{}",
        COLOR_GREEN, COLOR_RESET, COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  {}a{} test-files {}test1.py test2.py{}   # Runs: pytest test1.py test2.py",
        COLOR_GREEN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET
    );
    println!();

    println!("{}🎯 Pro Tips:{}", COLOR_BOLD, COLOR_RESET);
    println!(
        "  • Use {}$1, $2, $3{} to pass arguments to multiple commands in a chain",
        COLOR_GREEN, COLOR_RESET
    );
    println!(
        "  • Use {}$@{} to pass all arguments when you don't know how many there will be",
        COLOR_GREEN, COLOR_RESET
    );
    println!(
        "  • Use {}--parallel{} for independent tasks that can run simultaneously",
        COLOR_CYAN, COLOR_RESET
    );
    println!(
        "  • Combine {}--and{} and {}--or{} for robust deployment workflows",
        COLOR_GREEN, COLOR_RESET, COLOR_YELLOW, COLOR_RESET
    );
    println!(
        "  • Use {}--always{} for cleanup tasks that must run regardless",
        COLOR_BLUE, COLOR_RESET
    );
    println!(
        "  • {}--if-code{} enables sophisticated conditional logic",
        COLOR_CYAN, COLOR_RESET
    );
}

fn print_version() {
    println!(
        "{}{}🚀 Alias Manager v{}{}",
        COLOR_BOLD, COLOR_CYAN, VERSION, COLOR_RESET
    );
    println!(
        "{}⚡ A cross-platform command alias management tool written in Rust{}",
        COLOR_GRAY, COLOR_RESET
    );
    println!("{}🔗 Features: Advanced chaining, parallel execution, conditional logic, parameter substitution{}", COLOR_BLUE, COLOR_RESET);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let mut manager = match AliasManager::new() {
        Ok(mgr) => mgr,
        Err(e) => {
            eprintln!(
                "{}Error initializing alias manager:{} {}",
                COLOR_YELLOW, COLOR_RESET, e
            );
            std::process::exit(1);
        }
    };

    match args[1].as_str() {
        "--help" | "-h" => {
            print_help();
        }

        "--version" | "-v" => {
            print_version();
        }

        "--config" => {
            manager.show_config_location();
        }

        "--export" => {
            let target_path = if args.len() > 2 {
                Some(args[2].as_str())
            } else {
                None
            };

            match manager.export_config(target_path) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!(
                        "{}Error exporting config:{} {}",
                        COLOR_YELLOW, COLOR_RESET, e
                    );
                    std::process::exit(1);
                }
            }
        }

        "--push" => {
            // Optional: allow custom commit message only
            let mut message: Option<String> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--message" if i + 1 < args.len() => {
                        message = Some(args[i + 1].clone());
                        i += 2;
                    }
                    _ => {
                        eprintln!(
                            "{}Unknown or unsupported option for --push:{} {}",
                            COLOR_YELLOW, COLOR_RESET, args[i]
                        );
                        std::process::exit(1);
                    }
                }
            }

            match manager.push_config_to_github(message.as_deref()) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{}Error pushing config:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    std::process::exit(1);
                }
            }
        }

        "--pull" => {
            if args.len() > 2 {
                eprintln!(
                    "{}--pull does not accept options; repo is fixed.{}",
                    COLOR_YELLOW, COLOR_RESET
                );
                std::process::exit(1);
            }

            match manager.pull_config_from_github() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{}Error pulling config:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    std::process::exit(1);
                }
            }
        }

        "--add" => {
            if args.len() < 4 {
                eprintln!(
                    "{}Usage:{} a --add <n> <command> [OPTIONS]",
                    COLOR_YELLOW, COLOR_RESET
                );
                std::process::exit(1);
            }

            let name = args[2].clone();
            let first_command = args[3].clone();

            let mut description = None;
            let mut force = false;
            let mut parallel = false;
            let mut commands = vec![ChainCommand {
                command: first_command,
                operator: None, // First command has no operator
            }];

            let mut i = 4;
            while i < args.len() {
                match args[i].as_str() {
                    "--desc" => {
                        if i + 1 < args.len() {
                            description = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!(
                                "{}Error:{} --desc requires a description",
                                COLOR_YELLOW, COLOR_RESET
                            );
                            std::process::exit(1);
                        }
                    }
                    "--force" => {
                        force = true;
                        i += 1;
                    }
                    "--parallel" => {
                        parallel = true;
                        i += 1;
                    }
                    "--chain" | "--and" => {
                        if i + 1 < args.len() {
                            commands.push(ChainCommand {
                                command: args[i + 1].clone(),
                                operator: Some(ChainOperator::And),
                            });
                            i += 2;
                        } else {
                            eprintln!(
                                "{}Error:{} {} requires a command",
                                COLOR_YELLOW, COLOR_RESET, args[i]
                            );
                            std::process::exit(1);
                        }
                    }
                    "--or" => {
                        if i + 1 < args.len() {
                            commands.push(ChainCommand {
                                command: args[i + 1].clone(),
                                operator: Some(ChainOperator::Or),
                            });
                            i += 2;
                        } else {
                            eprintln!(
                                "{}Error:{} --or requires a command",
                                COLOR_YELLOW, COLOR_RESET
                            );
                            std::process::exit(1);
                        }
                    }
                    "--always" => {
                        if i + 1 < args.len() {
                            commands.push(ChainCommand {
                                command: args[i + 1].clone(),
                                operator: Some(ChainOperator::Always),
                            });
                            i += 2;
                        } else {
                            eprintln!(
                                "{}Error:{} --always requires a command",
                                COLOR_YELLOW, COLOR_RESET
                            );
                            std::process::exit(1);
                        }
                    }
                    "--if-code" => {
                        if i + 2 < args.len() {
                            match args[i + 1].parse::<i32>() {
                                Ok(code) => {
                                    commands.push(ChainCommand {
                                        command: args[i + 2].clone(),
                                        operator: Some(ChainOperator::IfCode(code)),
                                    });
                                    i += 3;
                                }
                                Err(_) => {
                                    eprintln!(
                                        "{}Error:{} --if-code requires a numeric exit code",
                                        COLOR_YELLOW, COLOR_RESET
                                    );
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            eprintln!(
                                "{}Error:{} --if-code requires an exit code and a command",
                                COLOR_YELLOW, COLOR_RESET
                            );
                            std::process::exit(1);
                        }
                    }
                    _ => {
                        eprintln!(
                            "{}Error:{} Unknown option '{}'",
                            COLOR_YELLOW, COLOR_RESET, args[i]
                        );
                        std::process::exit(1);
                    }
                }
            }

            // Determine if we should create a simple or complex command
            let command_type = if commands.len() == 1 && !parallel {
                // Single command, use simple type for backward compatibility
                CommandType::Simple(commands[0].command.clone())
            } else {
                // Multiple commands or parallel execution, use chain type
                CommandType::Chain(CommandChain { commands, parallel })
            };

            match manager.add_alias(name.clone(), command_type, description, force) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{}Error adding alias:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    std::process::exit(1);
                }
            }
        }

        "--list" => {
            let filter = if args.len() > 2 {
                Some(args[2].as_str())
            } else {
                None
            };
            manager.list_aliases(filter);
        }

        "--remove" => {
            if args.len() < 3 {
                eprintln!("{}Usage:{} a --remove <n>", COLOR_YELLOW, COLOR_RESET);
                std::process::exit(1);
            }

            match manager.remove_alias(&args[2]) {
                Ok(()) => println!("{}Removed alias '{}'{}", COLOR_GREEN, args[2], COLOR_RESET),
                Err(e) => {
                    eprintln!("{}Error removing alias:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    std::process::exit(1);
                }
            }
        }

        "--which" => {
            if args.len() < 3 {
                eprintln!("{}Usage:{} a --which <n>", COLOR_YELLOW, COLOR_RESET);
                std::process::exit(1);
            }

            manager.which_alias(&args[2]);
        }

        alias_name => {
            let alias_args = if args.len() > 2 { &args[2..] } else { &[] };

            match manager.execute_alias(alias_name, alias_args) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!(
                        "{}Error executing alias:{} {}",
                        COLOR_YELLOW, COLOR_RESET, e
                    );
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (AliasManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let manager = AliasManager {
            config: Config::new(),
            config_path,
        };

        (manager, temp_dir)
    }

    #[test]
    fn test_config_new() {
        let config = Config::new();
        assert!(config.aliases.is_empty());
    }

    #[test]
    fn test_add_alias() {
        let mut config = Config::new();

        let result = config.add_alias(
            "gst".to_string(),
            CommandType::Simple("git status".to_string()),
            Some("Quick status".to_string()),
            false,
        );

        assert!(result.is_ok());
        assert_eq!(config.aliases.len(), 1);

        let entry = config.get_alias("gst").unwrap();
        assert_eq!(entry.command_display(), "git status");
        assert_eq!(entry.description, Some("Quick status".to_string()));
    }

    #[test]
    fn test_add_alias_reserved_names() {
        let mut config = Config::new();

        let invalid_names = vec!["--add", "mgr:test", ".hidden"];

        for name in invalid_names {
            let result = config.add_alias(
                name.to_string(),
                CommandType::Simple("test command".to_string()),
                None,
                false,
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_remove_alias() {
        let mut config = Config::new();

        config
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false,
            )
            .unwrap();
        assert_eq!(config.aliases.len(), 1);

        let result = config.remove_alias("test");
        assert!(result.is_ok());
        assert_eq!(config.aliases.len(), 0);

        let result = config.remove_alias("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_alias() {
        let mut config = Config::new();
        config
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false,
            )
            .unwrap();

        let entry = config.get_alias("test");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().command_display(), "echo test");

        let entry = config.get_alias("nonexistent");
        assert!(entry.is_none());
    }

    #[test]
    fn test_list_aliases() {
        let mut config = Config::new();

        config
            .add_alias(
                "gst".to_string(),
                CommandType::Simple("git status".to_string()),
                None,
                false,
            )
            .unwrap();
        config
            .add_alias(
                "glog".to_string(),
                CommandType::Simple("git log".to_string()),
                None,
                false,
            )
            .unwrap();
        config
            .add_alias(
                "deploy".to_string(),
                CommandType::Simple("docker-compose up".to_string()),
                None,
                false,
            )
            .unwrap();

        let all_aliases = config.list_aliases(None);
        assert_eq!(all_aliases.len(), 3);

        let git_aliases = config.list_aliases(Some("g"));
        assert_eq!(git_aliases.len(), 2);

        let deploy_aliases = config.list_aliases(Some("deploy"));
        assert_eq!(deploy_aliases.len(), 1);
    }

    #[test]
    fn test_manager_save_load() {
        let (mut manager, _temp_dir) = create_test_manager();

        manager
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo hello".to_string()),
                Some("Test command".to_string()),
                false,
            )
            .unwrap();

        // Load a new manager from the saved config
        let loaded_manager = AliasManager {
            config: AliasManager::load_config(&manager.config_path).unwrap(),
            config_path: manager.config_path.clone(),
        };

        let entry = loaded_manager.config.get_alias("test").unwrap();
        assert_eq!(entry.command_display(), "echo hello");
        assert_eq!(entry.description, Some("Test command".to_string()));
    }

    #[test]
    fn test_manager_add_remove() {
        let (mut manager, _temp_dir) = create_test_manager();

        assert!(manager
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false
            )
            .is_ok());
        assert!(manager.config.get_alias("test").is_some());

        assert!(manager.remove_alias("test").is_ok());
        assert!(manager.config.get_alias("test").is_none());

        assert!(manager.remove_alias("nonexistent").is_err());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut config = Config::new();
        config
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                Some("Test".to_string()),
                false,
            )
            .unwrap();

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        let entry = deserialized.get_alias("test").unwrap();
        assert_eq!(entry.command_display(), "echo test");
        assert_eq!(entry.description, Some("Test".to_string()));
    }

    #[test]
    fn test_complex_chain() {
        let mut config = Config::new();

        let chain = CommandChain {
            commands: vec![
                ChainCommand {
                    command: "echo first".to_string(),
                    operator: None,
                },
                ChainCommand {
                    command: "echo second".to_string(),
                    operator: Some(ChainOperator::And),
                },
                ChainCommand {
                    command: "echo third".to_string(),
                    operator: Some(ChainOperator::Or),
                },
            ],
            parallel: false,
        };

        config
            .add_alias("test".to_string(), CommandType::Chain(chain), None, false)
            .unwrap();

        let entry = config.get_alias("test").unwrap();
        let display = entry.command_display();
        assert!(display.contains("&&"));
        assert!(display.contains("||"));
    }

    #[test]
    fn test_config_path_creation() {
        // This test verifies the path logic works, but doesn't actually create files
        // in the user's home directory during testing
        let path_result = AliasManager::get_config_path();
        assert!(path_result.is_ok());

        let path = path_result.unwrap();
        assert!(path.to_string_lossy().contains(".alias-mgr"));
        assert!(path.to_string_lossy().ends_with("config.json"));
    }

    #[test]
    fn test_substitute_parameters_positional() {
        let args = vec![
            "v1.0.0".to_string(),
            "main".to_string(),
            "feature".to_string(),
        ];

        // Test basic positional parameters
        assert_eq!(
            AliasManager::substitute_parameters("git tag $1", &args),
            "git tag v1.0.0"
        );
        assert_eq!(
            AliasManager::substitute_parameters("git checkout $2", &args),
            "git checkout main"
        );
        assert_eq!(
            AliasManager::substitute_parameters("git merge $2 $3", &args),
            "git merge main feature"
        );

        // Test out of bounds (should substitute with empty string)
        assert_eq!(
            AliasManager::substitute_parameters("git tag $5", &args),
            "git tag "
        );

        // Test $0 (should substitute with empty string - 1-indexed)
        assert_eq!(
            AliasManager::substitute_parameters("git tag $0", &args),
            "git tag "
        );
    }

    #[test]
    fn test_substitute_parameters_all_args() {
        let args = vec![
            "file1.txt".to_string(),
            "file2.txt".to_string(),
            "file3.txt".to_string(),
        ];

        // Test $@ (all arguments)
        assert_eq!(
            AliasManager::substitute_parameters("echo $@", &args),
            "echo file1.txt file2.txt file3.txt"
        );

        // Test $* (all arguments - same as $@ in our implementation)
        assert_eq!(
            AliasManager::substitute_parameters("echo $*", &args),
            "echo file1.txt file2.txt file3.txt"
        );

        // Test empty args
        let empty_args: Vec<String> = vec![];
        assert_eq!(
            AliasManager::substitute_parameters("echo $@", &empty_args),
            "echo "
        );
        assert_eq!(
            AliasManager::substitute_parameters("echo $*", &empty_args),
            "echo "
        );
    }

    #[test]
    fn test_substitute_parameters_escapes() {
        let args = vec!["value".to_string()];

        // Test literal dollar sign
        assert_eq!(
            AliasManager::substitute_parameters("echo $$", &args),
            "echo $"
        );
        assert_eq!(
            AliasManager::substitute_parameters("echo $$ $1", &args),
            "echo $ value"
        );

        // Test $ at end of string
        assert_eq!(
            AliasManager::substitute_parameters("echo $", &args),
            "echo $"
        );

        // Test $ followed by non-special character
        assert_eq!(
            AliasManager::substitute_parameters("echo $x", &args),
            "echo $x"
        );
        assert_eq!(
            AliasManager::substitute_parameters("echo $hello", &args),
            "echo $hello"
        );
    }

    #[test]
    fn test_substitute_parameters_complex() {
        let args = vec!["v1.0.0".to_string(), "origin".to_string()];

        // Test complex real-world example
        let command = "git tag $1 && git push $2 $1";
        let expected = "git tag v1.0.0 && git push origin v1.0.0";
        assert_eq!(
            AliasManager::substitute_parameters(command, &args),
            expected
        );

        // Test mixed variables and literals
        let command = "echo 'Version: $1, Remote: $2, Cost: $$5'";
        let expected = "echo 'Version: v1.0.0, Remote: origin, Cost: $5'";
        assert_eq!(
            AliasManager::substitute_parameters(command, &args),
            expected
        );
    }

    #[test]
    fn test_substitute_parameters_edge_cases() {
        let args = vec!["test".to_string()];

        // Test multiple consecutive variables
        assert_eq!(
            AliasManager::substitute_parameters("$1$1$1", &args),
            "testtesttest"
        );

        // Test variables at start and end
        assert_eq!(
            AliasManager::substitute_parameters("$1 middle $1", &args),
            "test middle test"
        );

        // Test only variables
        assert_eq!(AliasManager::substitute_parameters("$1", &args), "test");
        assert_eq!(AliasManager::substitute_parameters("$@", &args), "test");

        // Test no variables
        assert_eq!(
            AliasManager::substitute_parameters("echo hello", &args),
            "echo hello"
        );
    }

    #[test]
    fn test_has_parameter_variables() {
        // Test positional parameters
        assert!(AliasManager::has_parameter_variables("git tag $1"));
        assert!(AliasManager::has_parameter_variables("git push $2 $1"));
        assert!(AliasManager::has_parameter_variables("echo $9"));

        // Test special parameters
        assert!(AliasManager::has_parameter_variables("echo $@"));
        assert!(AliasManager::has_parameter_variables("echo $*"));

        // Test mixed content
        assert!(AliasManager::has_parameter_variables(
            "git tag $1 && git push origin $1"
        ));

        // Test no variables
        assert!(!AliasManager::has_parameter_variables("git status"));
        assert!(!AliasManager::has_parameter_variables("echo hello world"));

        // Test escaped dollar signs (should not count as variables)
        assert!(!AliasManager::has_parameter_variables("echo $$"));
        assert!(!AliasManager::has_parameter_variables("echo $$ literal"));

        // Test dollar followed by non-special chars
        assert!(!AliasManager::has_parameter_variables("echo $hello"));
        assert!(!AliasManager::has_parameter_variables("echo $abc"));

        // Test dollar at end
        assert!(!AliasManager::has_parameter_variables("echo $"));
    }

    #[test]
    fn test_parameter_substitution_integration() {
        let mut config = Config::new();

        // Test that command with variables is stored correctly
        let chain = CommandChain {
            commands: vec![
                ChainCommand {
                    command: "git tag $1".to_string(),
                    operator: None,
                },
                ChainCommand {
                    command: "git push origin $1".to_string(),
                    operator: Some(ChainOperator::And),
                },
            ],
            parallel: false,
        };

        config
            .add_alias(
                "tag-push".to_string(),
                CommandType::Chain(chain),
                Some("Tag and push".to_string()),
                false,
            )
            .unwrap();

        let entry = config.get_alias("tag-push").unwrap();
        let display = entry.command_display();
        assert!(display.contains("git tag $1"));
        assert!(display.contains("git push origin $1"));
    }

    #[test]
    fn test_export_config_to_current_dir() {
        let (mut manager, temp_dir) = create_test_manager();

        // Add some aliases to export
        manager
            .add_alias(
                "test1".to_string(),
                CommandType::Simple("echo test1".to_string()),
                None,
                false,
            )
            .unwrap();
        manager
            .add_alias(
                "test2".to_string(),
                CommandType::Simple("echo test2".to_string()),
                Some("Test 2".to_string()),
                false,
            )
            .unwrap();

        // Create a target directory within the temp directory
        let target_dir = temp_dir.path().join("export_test");
        fs::create_dir_all(&target_dir).unwrap();

        // Change to the target directory (simulate current directory)
        env::set_current_dir(&target_dir).unwrap();

        // Export config (should go to current directory)
        let result = manager.export_config(None);
        assert!(result.is_ok());

        // Verify the exported file exists and has correct content
        let exported_file = target_dir.join("config.json");
        assert!(exported_file.exists());

        // Load the exported config and verify it matches
        let exported_content = fs::read_to_string(&exported_file).unwrap();
        let exported_config: Config = serde_json::from_str(&exported_content).unwrap();

        assert_eq!(exported_config.aliases.len(), 2);
        assert!(exported_config.get_alias("test1").is_some());
        assert!(exported_config.get_alias("test2").is_some());
    }

    #[test]
    fn test_export_config_to_specified_dir() {
        let (mut manager, temp_dir) = create_test_manager();

        // Add an alias to export
        manager
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false,
            )
            .unwrap();

        // Create a target directory
        let target_dir = temp_dir.path().join("specified_target");

        // Export config to specified directory
        let result = manager.export_config(Some(target_dir.to_str().unwrap()));
        assert!(result.is_ok());

        // Verify the exported file exists
        let exported_file = target_dir.join("config.json");
        assert!(exported_file.exists());

        // Verify content
        let exported_content = fs::read_to_string(&exported_file).unwrap();
        let exported_config: Config = serde_json::from_str(&exported_content).unwrap();
        assert_eq!(exported_config.aliases.len(), 1);
    }

    #[test]
    fn test_export_config_creates_directory() {
        let (mut manager, temp_dir) = create_test_manager();

        // Add an alias to export
        manager
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false,
            )
            .unwrap();

        // Specify non-existent target directory
        let target_dir = temp_dir
            .path()
            .join("non_existent")
            .join("nested")
            .join("dir");

        // Export should create the directory structure
        let result = manager.export_config(Some(target_dir.to_str().unwrap()));
        assert!(result.is_ok());

        // Verify directory was created and file exists
        assert!(target_dir.exists());
        assert!(target_dir.is_dir());
        assert!(target_dir.join("config.json").exists());
    }

    #[test]
    fn test_export_config_no_source() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent_config.json");

        let manager = AliasManager {
            config: Config::new(),
            config_path,
        };

        let target_dir = temp_dir.path().join("target");
        let result = manager.export_config(Some(target_dir.to_str().unwrap()));

        // Should fail because source config doesn't exist
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Source config file does not exist"));
    }

    #[test]
    fn test_export_config_target_is_file() {
        let (mut manager, temp_dir) = create_test_manager();

        // Add an alias to export
        manager
            .add_alias(
                "test".to_string(),
                CommandType::Simple("echo test".to_string()),
                None,
                false,
            )
            .unwrap();

        // Create a file at the target path (not a directory)
        let target_file = temp_dir.path().join("existing_file.txt");
        fs::write(&target_file, "existing content").unwrap();

        // Export should fail because target exists and is not a directory
        let result = manager.export_config(Some(target_file.to_str().unwrap()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("exists but is not a directory"));
    }
}
