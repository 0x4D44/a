use base64::Engine;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const VERSION: &str = "1.4.0";
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

trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String]) -> Result<i32, String>;
}

#[derive(Default)]
struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<i32, String> {
        let mut cmd = Command::new(program);
        cmd.args(args);

        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to execute command '{}': {}", program, e))?;

        Ok(status.code().unwrap_or(1))
    }
}

#[derive(Clone, Debug)]
struct GitHubResponse {
    status: u16,
    body: Option<String>,
    json: Option<serde_json::Value>,
}

impl GitHubResponse {
    #[cfg(test)]
    fn from_status(status: u16) -> Self {
        Self {
            status,
            body: None,
            json: None,
        }
    }

    fn from_text(status: u16, text: String) -> Self {
        let json = serde_json::from_str(&text).ok();
        Self {
            status,
            body: Some(text),
            json,
        }
    }

    #[cfg(test)]
    fn from_json(status: u16, json: serde_json::Value) -> Self {
        Self {
            status,
            body: None,
            json: Some(json),
        }
    }

    fn status(&self) -> u16 {
        self.status
    }

    fn json(&self) -> Option<&serde_json::Value> {
        self.json.as_ref()
    }

    fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

trait GitHubClient: Send + Sync {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<GitHubResponse, String>;
    fn put(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: serde_json::Value,
    ) -> Result<GitHubResponse, String>;
}

#[derive(Clone)]
struct UreqGitHubClient {
    agent: ureq::Agent,
}

impl Default for UreqGitHubClient {
    fn default() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        Self { agent }
    }
}

impl UreqGitHubClient {
    #[cfg(test)]
    fn with_agent(agent: ureq::Agent) -> Self {
        Self { agent }
    }
}

impl GitHubClient for UreqGitHubClient {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<GitHubResponse, String> {
        let mut request = self.agent.get(url);
        for (key, value) in headers {
            request = request.set(key, value);
        }

        match request.call() {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.into_string().unwrap_or_default();
                Ok(GitHubResponse::from_text(status, text))
            }
            Err(ureq::Error::Status(status, resp)) => {
                let text = resp.into_string().unwrap_or_default();
                Ok(GitHubResponse::from_text(status as u16, text))
            }
            Err(e) => Err(format!("Failed to perform GitHub GET: {}", e)),
        }
    }

    fn put(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: serde_json::Value,
    ) -> Result<GitHubResponse, String> {
        let mut request = self.agent.put(url);
        for (key, value) in headers {
            request = request.set(key, value);
        }

        match request.send_json(body) {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.into_string().unwrap_or_default();
                Ok(GitHubResponse::from_text(status, text))
            }
            Err(ureq::Error::Status(status, resp)) => {
                let text = resp.into_string().unwrap_or_default();
                Ok(GitHubResponse::from_text(status as u16, text))
            }
            Err(e) => Err(format!("Failed to perform GitHub PUT: {}", e)),
        }
    }
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
    command_runner: Arc<dyn CommandRunner + Send + Sync>,
    github_client: Arc<dyn GitHubClient + Send + Sync>,
}

impl AliasManager {
    fn new() -> Result<Self, String> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_config(&config_path)?;

        let runner: Arc<dyn CommandRunner + Send + Sync> = Arc::new(SystemCommandRunner::default());
        let github: Arc<dyn GitHubClient + Send + Sync> = Arc::new(UreqGitHubClient::default());

        Ok(Self::with_dependencies(config, config_path, runner, github))
    }

    fn with_dependencies(
        config: Config,
        config_path: PathBuf,
        command_runner: Arc<dyn CommandRunner + Send + Sync>,
        github_client: Arc<dyn GitHubClient + Send + Sync>,
    ) -> Self {
        AliasManager {
            config,
            config_path,
            command_runner,
            github_client,
        }
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
        // 1) Environment variables
        if let Ok(tok) = env::var("A_GITHUB_TOKEN") {
            if !tok.trim().is_empty() {
                return Some(tok);
            }
        }
        if let Ok(tok) = env::var("GITHUB_TOKEN") {
            if !tok.trim().is_empty() {
                return Some(tok);
            }
        }
        if let Ok(tok) = env::var("GH_TOKEN") {
            if !tok.trim().is_empty() {
                return Some(tok);
            }
        }

        // 2) GitHub CLI (gh) – try status first (non-interactive), then token
        if let Some(tok) = Self::github_token_from_gh_status() {
            return Some(tok);
        }
        if let Some(tok) = Self::github_token_from_gh_token() {
            return Some(tok);
        }

        // 3) Git credential helper (may have PAT stored as the password)
        if let Some(tok) = Self::github_token_from_git_credentials("github.com") {
            return Some(tok);
        }
        if let Some(tok) = Self::github_token_from_git_credentials("api.github.com") {
            return Some(tok);
        }

        None
    }

    fn github_token_from_gh_status() -> Option<String> {
        let mut cmd = Command::new("gh");
        cmd.arg("auth")
            .arg("status")
            .arg("--show-token")
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let output = match cmd.output() {
            Ok(o) => o,
            Err(_) => return None,
        };
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Look for a line like: "Token: ghp_..."
        for line in stdout.lines() {
            if let Some(idx) = line.find("Token:") {
                let token_part = line[idx + "Token:".len()..].trim();
                if !token_part.is_empty() && token_part != "<TOKEN>" {
                    return Some(token_part.to_string());
                }
            }
        }
        None
    }

    fn github_token_from_gh_token() -> Option<String> {
        let mut cmd = Command::new("gh");
        cmd.arg("auth")
            .arg("token")
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let output = match cmd.output() {
            Ok(o) => o,
            Err(_) => return None,
        };
        if !output.status.success() {
            return None;
        }
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }

    fn github_token_from_git_credentials(host: &str) -> Option<String> {
        let mut cmd = Command::new("git");
        cmd.arg("credential")
            .arg("fill")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => return None,
        };

        if let Some(mut stdin) = child.stdin.take() {
            // Standard input format for git credential helper
            let _ = write!(stdin, "protocol=https\nhost={}\n\n", host);
        }

        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(_) => return None,
        };

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse key=value lines; prefer password as token
        let mut password: Option<String> = None;
        for line in stdout.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if k == "password" && !v.trim().is_empty() {
                    password = Some(v.trim().to_string());
                    break;
                }
            }
        }
        password
    }

    fn push_config_to_github(&self, message: Option<&str>) -> Result<(), String> {
        let repo = GITHUB_REPO;
        let branch = GITHUB_BRANCH;
        let path_in_repo = GITHUB_CONFIG_PATH;
        let commit_message = message.unwrap_or("chore(config): update alias config");

        let token = Self::github_token().ok_or_else(|| {
            "Missing GitHub token. Set A_GITHUB_TOKEN/GITHUB_TOKEN/GH_TOKEN or login via gh/git.".to_string()
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

        let headers = vec![
            ("User-Agent", "a-alias-manager".to_string()),
            ("Authorization", format!("Bearer {}", token)),
        ];

        let mut maybe_sha: Option<String> = None;
        let get_response = self.github_client.get(&get_url, &headers)?;
        match get_response.status() {
            200 => {
                if let Some(json) = get_response.json() {
                    if let Some(sha) = json.get("sha").and_then(|v| v.as_str()) {
                        maybe_sha = Some(sha.to_string());
                    }
                }
            }
            404 => {}
            status => {
                return Err(format!("Failed to query existing file: status {}", status));
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

        let put_response = self.github_client.put(&api_base, &headers, body)?;

        if put_response.status() == 200 || put_response.status() == 201 {
            println!(
                "{}Config pushed to GitHub:{} https://github.com/{}/blob/{}/{}",
                COLOR_GREEN, COLOR_RESET, repo, branch, path_in_repo
            );
            Ok(())
        } else {
            Err(format!(
                "GitHub API returned status {}",
                put_response.status()
            ))
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
        let mut headers = vec![("User-Agent", "a-alias-manager".to_string())];
        if let Some(token) = &token_opt {
            headers.push(("Authorization", format!("Bearer {}", token)));
        }

        let response = self.github_client.get(&api_url, &headers)?;
        if response.status() != 200 {
            return Err(format!("GitHub API returned status {}", response.status()));
        }

        let val = response
            .json()
            .cloned()
            .or_else(|| {
                response
                    .body()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            })
            .ok_or_else(|| "Failed to parse GitHub response".to_string())?;

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
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let mut reader = stdin.lock();
        Self::confirm_overwrite_with_reader(&mut reader, &mut stdout)
    }

    fn confirm_overwrite_with_reader<R, W>(reader: &mut R, writer: &mut W) -> Result<bool, String>
    where
        R: io::BufRead,
        W: Write,
    {
        write!(writer, "{}Overwrite? (y/N):{} ", COLOR_YELLOW, COLOR_RESET)
            .map_err(|e| format!("Failed to write prompt: {}", e))?;
        writer
            .flush()
            .map_err(|e| format!("Failed to flush stdout: {}", e))?;

        let mut input = String::new();
        reader
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
            let runner = self.command_runner.clone();

            let handle = thread::spawn(move || {
                let result = AliasManager::execute_with_runner(runner, cmd, args);
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
        let (program, command_args) = Self::prepare_command_invocation(command_str, args)?;

        self.command_runner.run(&program, &command_args)
    }

    fn execute_single_command(&self, command_str: &str, args: &[String]) -> Result<(), String> {
        let (program, command_args) = Self::prepare_command_invocation(command_str, args)?;

        let exit_code = self.command_runner.run(&program, &command_args)?;

        if exit_code != 0 {
            std::process::exit(exit_code);
        }

        Ok(())
    }

    fn execute_with_runner(
        runner: Arc<dyn CommandRunner + Send + Sync>,
        command_str: String,
        args: Vec<String>,
    ) -> Result<i32, String> {
        let (program, command_args) =
            AliasManager::prepare_command_invocation(&command_str, &args)?;
        runner.run(&program, &command_args)
    }
    fn prepare_command_invocation(
        command_str: &str,
        args: &[String],
    ) -> Result<(String, Vec<String>), String> {
        let has_params = Self::has_parameter_variables(command_str);
        let resolved_command = if has_params {
            Self::substitute_parameters(command_str, args)
        } else {
            command_str.to_string()
        };

        let mut tokens = shell_words::split(&resolved_command)
            .map_err(|e| format!("Failed to parse command '{}': {}", resolved_command, e))?;

        if tokens.is_empty() {
            return Err("Empty command in alias".to_string());
        }

        let program = tokens.remove(0);

        if !has_params {
            tokens.extend(args.iter().cloned());
        }

        Ok((program, tokens))
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
                            // $N -> Nth argument (1-indexed), support multi-digit
                            let mut number = String::new();
                            while let Some(&digit_ch) = chars.peek() {
                                if digit_ch.is_ascii_digit() {
                                    number.push(digit_ch);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }

                            if let Ok(index) = number.parse::<usize>() {
                                if index > 0 && index <= args.len() {
                                    result.push_str(&args[index - 1]);
                                }
                                // If index is 0 or out of bounds, substitute with empty string
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

fn print_help(show_examples: bool) {
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
    println!(
        "  {}a{} {}--help --examples{}          Show help with detailed examples",
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

    if show_examples {
        print_examples();
    } else {
        println!(
            "{}💡 Tip:{} Run {}a --help --examples{} to view detailed workflows",
            COLOR_CYAN, COLOR_RESET, COLOR_GREEN, COLOR_RESET
        );
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
        print_help(false);
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
            let mut show_examples = false;
            for extra in &args[2..] {
                match extra.as_str() {
                    "--examples" => show_examples = true,
                    "--no-examples" => show_examples = false,
                    _ => {
                        eprintln!(
                            "{}Unknown option for --help:{} {}",
                            COLOR_YELLOW, COLOR_RESET, extra
                        );
                        std::process::exit(1);
                    }
                }
            }
            print_help(show_examples);
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
    use std::collections::VecDeque;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::io::{self, Cursor, Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[derive(Default)]
    struct MockCommandRunner {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        responses: Mutex<VecDeque<Result<i32, String>>>,
    }

    impl MockCommandRunner {
        fn new() -> Self {
            Self::default()
        }

        fn with_responses(responses: Vec<Result<i32, String>>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }

        fn push_response(&self, response: Result<i32, String>) {
            self.responses.lock().unwrap().push_back(response);
        }

        fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn run(&self, program: &str, args: &[String]) -> Result<i32, String> {
            self.calls
                .lock()
                .unwrap()
                .push((program.to_string(), args.to_vec()));

            if let Some(result) = self.responses.lock().unwrap().pop_front() {
                result
            } else {
                Ok(0)
            }
        }
    }

    #[derive(Default)]
    struct MockGitHubClient {
        requests: Mutex<Vec<GitHubRequest>>,
        responses: Mutex<VecDeque<Result<GitHubResponse, String>>>,
    }

    #[derive(Clone, Debug)]
    struct GitHubRequest {
        method: String,
        _url: String,
        headers: Vec<(String, String)>,
        body: Option<serde_json::Value>,
    }

    impl MockGitHubClient {
        fn new() -> Self {
            Self::default()
        }

        fn with_responses(responses: Vec<Result<GitHubResponse, String>>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }

        fn requests(&self) -> Vec<GitHubRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl GitHubClient for MockGitHubClient {
        fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<GitHubResponse, String> {
            self.requests.lock().unwrap().push(GitHubRequest {
                method: "GET".to_string(),
                _url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
                body: None,
            });

            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(GitHubResponse::from_status(200)))
        }

        fn put(
            &self,
            url: &str,
            headers: &[(&str, String)],
            body: serde_json::Value,
        ) -> Result<GitHubResponse, String> {
            self.requests.lock().unwrap().push(GitHubRequest {
                method: "PUT".to_string(),
                _url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
                body: Some(body.clone()),
            });

            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(GitHubResponse::from_status(200)))
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn http_response(status: u16, reason: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            reason,
            body.len(),
            body
        )
    }

    fn spawn_stub_server(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                stream
                    .write_all(response.as_bytes())
                    .expect("send stub response");
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn spawn_drop_server() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                // Drop without sending a response to trigger transport error.
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn create_manager_with_mocks(
        command_responses: Vec<Result<i32, String>>,
        github_responses: Vec<Result<GitHubResponse, String>>,
    ) -> (
        AliasManager,
        TempDir,
        Arc<MockCommandRunner>,
        Arc<MockGitHubClient>,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let runner = Arc::new(MockCommandRunner::with_responses(command_responses));
        let github = Arc::new(MockGitHubClient::with_responses(github_responses));

        let runner_trait: Arc<dyn CommandRunner + Send + Sync> = runner.clone();
        let github_trait: Arc<dyn GitHubClient + Send + Sync> = github.clone();

        let manager =
            AliasManager::with_dependencies(Config::new(), config_path, runner_trait, github_trait);

        (manager, temp_dir, runner, github)
    }

    fn create_test_manager() -> (AliasManager, TempDir) {
        let (manager, temp_dir, _runner, _github) =
            create_manager_with_mocks(Vec::new(), Vec::new());
        (manager, temp_dir)
    }

    struct WorkingDirGuard {
        original: PathBuf,
    }

    impl WorkingDirGuard {
        fn change_to(target: &Path) -> io::Result<Self> {
            let original = env::current_dir()?;
            env::set_current_dir(target)?;
            Ok(Self { original })
        }
    }

    impl Drop for WorkingDirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    struct EnvVarGuard {
        key: String,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set<K, V>(key: K, value: V) -> Self
        where
            K: Into<String>,
            V: AsRef<OsStr>,
        {
            let key_string = key.into();
            let original = env::var_os(&key_string);
            env::set_var(&key_string, value.as_ref());
            Self {
                key: key_string,
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => env::set_var(&self.key, val),
                None => env::remove_var(&self.key),
            }
        }
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
        let loaded_runner: Arc<dyn CommandRunner + Send + Sync> =
            Arc::new(MockCommandRunner::new());
        let loaded_github: Arc<dyn GitHubClient + Send + Sync> = Arc::new(MockGitHubClient::new());
        let loaded_manager = AliasManager::with_dependencies(
            AliasManager::load_config(&manager.config_path).unwrap(),
            manager.config_path.clone(),
            loaded_runner,
            loaded_github,
        );

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
        let temp_dir = TempDir::new().unwrap();
        let _env_guard = env_lock().lock().unwrap();
        let _home_guard = EnvVarGuard::set("HOME", temp_dir.path());
        let _userprofile_guard = EnvVarGuard::set("USERPROFILE", temp_dir.path());

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
    fn test_substitute_parameters_multi_digit() {
        let args = (1..=12).map(|i| format!("val{}", i)).collect::<Vec<_>>();
        assert_eq!(
            AliasManager::substitute_parameters("echo $10", &args),
            "echo val10"
        );
        assert_eq!(
            AliasManager::substitute_parameters("$12-$1", &args),
            "val12-val1"
        );
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
        let _dir_guard = WorkingDirGuard::change_to(&target_dir).unwrap();

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

        let runner: Arc<dyn CommandRunner + Send + Sync> = Arc::new(MockCommandRunner::new());
        let github: Arc<dyn GitHubClient + Send + Sync> = Arc::new(MockGitHubClient::new());
        let manager = AliasManager::with_dependencies(Config::new(), config_path, runner, github);

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

    #[test]
    fn test_github_token_precedence() {
        let _env_guard = env_lock().lock().unwrap();
        let _gh_guard = EnvVarGuard::set("GH_TOKEN", "third");
        let _git_guard = EnvVarGuard::set("GITHUB_TOKEN", "second");
        let _a_guard = EnvVarGuard::set("A_GITHUB_TOKEN", "first");

        assert_eq!(AliasManager::github_token().as_deref(), Some("first"));
    }

    #[test]
    fn test_push_config_to_github_updates_existing_file() {
        let _env_guard = env_lock().lock().unwrap();
        let responses = vec![
            Ok(GitHubResponse::from_json(
                200,
                serde_json::json!({"sha": "existing-sha"}),
            )),
            Ok(GitHubResponse::from_status(200)),
        ];
        let (manager, _temp_dir, _runner, github) =
            create_manager_with_mocks(Vec::new(), responses);

        fs::write(&manager.config_path, r#"{"aliases":{}}"#).unwrap();
        let _token_guard = EnvVarGuard::set("A_GITHUB_TOKEN", "test-token");

        manager
            .push_config_to_github(Some("test message"))
            .expect("push succeeds");

        let requests = github.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        let put_request = requests
            .iter()
            .find(|req| req.method == "PUT")
            .expect("PUT request captured");
        let body = put_request.body.as_ref().expect("body present");
        assert_eq!(body["message"], "test message");
        assert_eq!(body["sha"], "existing-sha");
        assert!(put_request
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "Bearer test-token"));
    }

    #[test]
    fn test_push_config_to_github_creates_file_when_missing() {
        let _env_guard = env_lock().lock().unwrap();
        let responses = vec![
            Ok(GitHubResponse::from_status(404)),
            Ok(GitHubResponse::from_status(201)),
        ];
        let (manager, _temp_dir, _runner, github) =
            create_manager_with_mocks(Vec::new(), responses);

        fs::write(&manager.config_path, r#"{"aliases":{}}"#).unwrap();
        let _token_guard = EnvVarGuard::set("A_GITHUB_TOKEN", "push-token");

        manager.push_config_to_github(None).expect("push succeeds");

        let requests = github.requests();
        assert_eq!(requests.len(), 2);
        let body = requests
            .iter()
            .find(|req| req.method == "PUT")
            .and_then(|req| req.body.as_ref())
            .expect("body present");
        assert_eq!(
            body["message"],
            serde_json::Value::String("chore(config): update alias config".to_string())
        );
        assert!(body.get("sha").is_none());
    }

    #[test]
    fn test_push_config_to_github_propagates_failure() {
        let _env_guard = env_lock().lock().unwrap();
        let responses = vec![
            Ok(GitHubResponse::from_status(404)),
            Ok(GitHubResponse::from_status(500)),
        ];
        let (manager, _temp_dir, _runner, _github) =
            create_manager_with_mocks(Vec::new(), responses);

        fs::write(&manager.config_path, r#"{"aliases":{}}"#).unwrap();
        let _token_guard = EnvVarGuard::set("A_GITHUB_TOKEN", "push-token");

        let err = manager
            .push_config_to_github(None)
            .expect_err("push should fail");
        assert!(err.contains("GitHub API returned status 500"));
    }

    #[test]
    fn test_pull_config_from_github_writes_file_and_backup() {
        let _env_guard = env_lock().lock().unwrap();
        let new_config = r#"{"aliases":{"remote":{"command_type":{"Simple":"echo remote"},"description":null,"created":"2025-10-20"}}}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(new_config);
        let responses = vec![Ok(GitHubResponse::from_json(
            200,
            serde_json::json!({
                "encoding": "base64",
                "content": encoded
            }),
        ))];
        let (mut manager, temp_dir, _runner, github) =
            create_manager_with_mocks(Vec::new(), responses);

        let existing_config = r#"{"aliases":{"local":{"command_type":{"Simple":"echo local"},"description":null,"created":"2025-01-01"}}}"#;
        fs::write(&manager.config_path, existing_config).unwrap();
        let backup_path = manager
            .config_path
            .parent()
            .unwrap()
            .join("config.backup.json");

        let _token_guard = EnvVarGuard::set("GITHUB_TOKEN", "pull-token");

        manager.pull_config_from_github().expect("pull succeeds");

        assert!(backup_path.exists());
        let written = fs::read_to_string(&manager.config_path).unwrap();
        assert_eq!(written, new_config);
        assert!(manager.config.aliases.contains_key("remote"));

        let requests = github.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "Bearer pull-token"));

        let _ = fs::remove_file(temp_dir.path().join("config.backup.json"));
    }

    #[test]
    fn test_pull_config_from_github_invalid_encoding_errors() {
        let _env_guard = env_lock().lock().unwrap();
        let responses = vec![Ok(GitHubResponse::from_json(
            200,
            serde_json::json!({
                "encoding": "utf-8",
                "content": "not-base64"
            }),
        ))];
        let (mut manager, _temp_dir, _runner, _github) =
            create_manager_with_mocks(Vec::new(), responses);

        let err = manager
            .pull_config_from_github()
            .expect_err("pull should fail");
        assert!(err.contains("Unsupported encoding"));
    }

    #[test]
    fn test_execute_with_real_runner_success() {
        let _env_guard = env_lock().lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        fs::write(&config_path, r#"{"aliases":{}}"#).unwrap();

        let runner: Arc<dyn CommandRunner + Send + Sync> = Arc::new(SystemCommandRunner::default());
        let github: Arc<dyn GitHubClient + Send + Sync> = Arc::new(MockGitHubClient::new());
        let manager = AliasManager::with_dependencies(Config::new(), config_path, runner, github);

        #[cfg(windows)]
        let command = "cmd /C exit 0";
        #[cfg(not(windows))]
        let command = "true";

        let exit = manager
            .execute_single_command_with_exit_code(command, &[])
            .expect("command succeeds");
        assert_eq!(exit, 0);
    }

    #[test]
    fn test_execute_with_real_runner_failure() {
        let _env_guard = env_lock().lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        fs::write(&config_path, r#"{"aliases":{}}"#).unwrap();

        let runner: Arc<dyn CommandRunner + Send + Sync> = Arc::new(SystemCommandRunner::default());
        let github: Arc<dyn GitHubClient + Send + Sync> = Arc::new(MockGitHubClient::new());
        let manager = AliasManager::with_dependencies(Config::new(), config_path, runner, github);

        let err = manager
            .execute_single_command_with_exit_code("definitely-not-a-real-binary", &[])
            .expect_err("expected failure");
        assert!(err.contains("Failed to execute command"));
    }

    #[test]
    fn test_print_help_and_examples() {
        // Calls are captured by the test harness, keeping stdout noise minimal.
        print_help(false);
        print_help(true);
    }

    #[test]
    fn test_print_version() {
        print_version();
    }

    #[test]
    fn test_ureq_github_client_get_success() {
        let body = r#"{"sha":"abc"}"#;
        let (url, handle) = spawn_stub_server(vec![http_response(200, "OK", body)]);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build();
        let client = UreqGitHubClient::with_agent(agent);

        let response = client
            .get(
                &format!("{}/content", url),
                &[("User-Agent", "test".into())],
            )
            .expect("request succeeds");
        assert_eq!(response.status(), 200);
        assert_eq!(response.json().unwrap()["sha"], "abc");

        handle.join().unwrap();
    }

    #[test]
    fn test_ureq_github_client_get_not_found() {
        let body = r#"{"message":"not found"}"#;
        let (url, handle) = spawn_stub_server(vec![http_response(404, "Not Found", body)]);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build();
        let client = UreqGitHubClient::with_agent(agent);

        let response = client
            .get(
                &format!("{}/missing", url),
                &[("User-Agent", "test".into())],
            )
            .expect("request succeeds");
        assert_eq!(response.status(), 404);
        assert!(response.body().unwrap().contains("not found"));

        handle.join().unwrap();
    }

    #[test]
    fn test_ureq_github_client_put_success() {
        let responses = vec![http_response(201, "Created", r#"{"ok":true}"#)];
        let (url, handle) = spawn_stub_server(responses);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build();
        let client = UreqGitHubClient::with_agent(agent);

        let response = client
            .put(
                &format!("{}/update", url),
                [("User-Agent", "test".to_string())].as_ref(),
                serde_json::json!({"message":"hi"}),
            )
            .expect("request succeeds");
        assert_eq!(response.status(), 201);

        handle.join().unwrap();
    }

    #[test]
    fn test_ureq_github_client_get_transport_error() {
        let (url, handle) = spawn_drop_server();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build();
        let client = UreqGitHubClient::with_agent(agent);

        let err = client
            .get(&format!("{}/drop", url), &[("User-Agent", "test".into())])
            .expect_err("expected transport error");
        assert!(err.contains("Failed to perform GitHub GET"));

        handle.join().unwrap();
    }

    #[test]
    fn test_ureq_github_client_put_transport_error() {
        let (url, handle) = spawn_drop_server();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build();
        let client = UreqGitHubClient::with_agent(agent);

        let err = client
            .put(
                &format!("{}/update", url),
                [("User-Agent", "test".to_string())].as_ref(),
                serde_json::json!({"message":"hi"}),
            )
            .expect_err("expected transport error");
        assert!(err.contains("Failed to perform GitHub PUT"));

        handle.join().unwrap();
    }

    #[test]
    fn test_system_command_runner_success() {
        let runner = SystemCommandRunner::default();
        #[cfg(windows)]
        let args = vec!["/C".to_string(), "exit 0".to_string()];
        #[cfg(not(windows))]
        let args: Vec<String> = Vec::new();

        #[cfg(windows)]
        let program = "cmd";
        #[cfg(not(windows))]
        let program = "true";

        let exit = runner.run(program, &args).expect("command succeeds");
        assert_eq!(exit, 0);
    }

    #[test]
    fn test_system_command_runner_missing_program_errors() {
        let runner = SystemCommandRunner::default();
        let err = runner
            .run("definitely-not-a-real-binary", &[])
            .expect_err("expected failure");
        assert!(err.contains("Failed to execute command"));
    }

    #[test]
    fn test_prepare_command_invocation_handles_quoted_args() {
        let args: Vec<String> = Vec::new();
        let (program, command_args) =
            AliasManager::prepare_command_invocation("git commit -m \"fix login flow\"", &args)
                .unwrap();

        assert_eq!(program, "git");
        assert_eq!(
            command_args,
            vec![
                "commit".to_string(),
                "-m".to_string(),
                "fix login flow".to_string()
            ]
        );
    }

    #[test]
    fn test_migrate_legacy_config_preserves_aliases() {
        let legacy = r#"
        {
            "aliases": {
                "gst": {
                    "command": "git status",
                    "description": "Quick status",
                    "created": "2024-01-01"
                }
            }
        }
        "#;

        let config = AliasManager::migrate_legacy_config(legacy).expect("migrate legacy");
        let entry = config.aliases.get("gst").expect("alias migrated");

        match &entry.command_type {
            CommandType::Simple(cmd) => assert_eq!(cmd, "git status"),
            other => panic!("unexpected command type: {:?}", other),
        }
        assert_eq!(entry.description.as_deref(), Some("Quick status"));
        assert_eq!(entry.created, "2024-01-01");
    }

    #[test]
    fn test_load_config_migrates_legacy_file() {
        let temp_dir = TempDir::new().unwrap();
        let legacy_path = temp_dir.path().join("legacy.json");
        let legacy = r#"
        {
            "aliases": {
                "build": {
                    "command": "npm run build",
                    "description": null,
                    "created": "2023-12-12"
                }
            }
        }
        "#;
        fs::write(&legacy_path, legacy).unwrap();

        let config = AliasManager::load_config(&legacy_path).expect("load config");
        let entry = config.aliases.get("build").expect("build alias present");
        match &entry.command_type {
            CommandType::Simple(cmd) => assert_eq!(cmd, "npm run build"),
            other => panic!("unexpected command type: {:?}", other),
        }
        assert_eq!(entry.description, None);
    }

    #[test]
    fn test_confirm_overwrite_yes() {
        let mut reader = Cursor::new(b"y\n".to_vec());
        let mut output = Vec::new();
        let result = AliasManager::confirm_overwrite_with_reader(&mut reader, &mut output).unwrap();
        assert!(result);
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("Overwrite?"));
    }

    #[test]
    fn test_confirm_overwrite_no_default() {
        let mut reader = Cursor::new(b"\n".to_vec());
        let mut output = Vec::new();
        let result = AliasManager::confirm_overwrite_with_reader(&mut reader, &mut output).unwrap();
        assert!(!result);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::Other, "cannot write"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_confirm_overwrite_write_error() {
        let mut reader = Cursor::new(b"y\n".to_vec());
        let mut writer = FailingWriter;
        let err = AliasManager::confirm_overwrite_with_reader(&mut reader, &mut writer)
            .expect_err("expected write failure");
        assert!(err.contains("Failed to write prompt"));
    }

    #[test]
    fn test_execute_alias_simple_runs_command() {
        let (mut manager, _temp_dir, runner, _github) =
            create_manager_with_mocks(Vec::new(), Vec::new());
        runner.push_response(Ok(0));

        manager
            .add_alias(
                "hello".to_string(),
                CommandType::Simple("echo hello".to_string()),
                None,
                false,
            )
            .unwrap();

        manager.execute_alias("hello", &[]).unwrap();

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "echo");
        assert_eq!(calls[0].1, vec!["hello"]);
    }

    #[test]
    fn test_execute_sequential_chain_respects_conditions() {
        let (manager, _temp_dir, runner, _github) =
            create_manager_with_mocks(vec![Ok(0), Ok(1), Ok(0)], Vec::new());

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

        manager
            .execute_sequential_chain(&chain, &[])
            .expect("sequential chain succeeds");

        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "echo");
        assert_eq!(calls[1].0, "echo");
        assert_eq!(calls[2].0, "echo");
    }

    #[test]
    fn test_execute_parallel_chain_reports_failures() {
        let (manager, _temp_dir, runner, _github) =
            create_manager_with_mocks(vec![Ok(0), Err("boom".to_string()), Ok(0)], Vec::new());

        let chain = CommandChain {
            commands: vec![
                ChainCommand {
                    command: "echo alpha".to_string(),
                    operator: None,
                },
                ChainCommand {
                    command: "echo beta".to_string(),
                    operator: None,
                },
                ChainCommand {
                    command: "echo gamma".to_string(),
                    operator: None,
                },
            ],
            parallel: true,
        };

        let err = manager
            .execute_parallel_chain(&chain, &[])
            .expect_err("parallel chain should fail");
        assert!(err.contains("parallel commands failed"));

        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
    }
}
