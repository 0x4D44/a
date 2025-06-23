use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const VERSION: &str = "1.0.0";

// ANSI color codes
const COLOR_RESET: &str = "\x1b[0m";
const COLOR_BOLD: &str = "\x1b[1m";
const COLOR_GREEN: &str = "\x1b[32m";
const COLOR_BLUE: &str = "\x1b[34m";
const COLOR_CYAN: &str = "\x1b[36m";
const COLOR_YELLOW: &str = "\x1b[33m";
const COLOR_GRAY: &str = "\x1b[90m";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AliasEntry {
    command: String,
    description: Option<String>,
    created: String,
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

    fn add_alias(&mut self, name: String, command: String, description: Option<String>, force: bool) -> Result<bool, String> {
        if name.starts_with("--") || name.contains("mgr:") || name.starts_with(".") {
            return Err(format!("Invalid alias name '{}': cannot use reserved prefixes", name));
        }

        let is_overwrite = self.aliases.contains_key(&name);
        if is_overwrite && !force {
            return Ok(false); // Signal that confirmation is needed
        }

        let entry = AliasEntry {
            command,
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
        
        Ok(AliasManager { config, config_path })
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

        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))
    }

    fn save_config(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        fs::write(&self.config_path, content)
            .map_err(|e| format!("Failed to save config file: {}", e))
    }

    fn add_alias(&mut self, name: String, command: String, description: Option<String>, force: bool) -> Result<(), String> {
        // Check if alias already exists before making changes
        let alias_existed = self.config.aliases.contains_key(&name);
        
        // Check if alias exists and get confirmation if needed
        let confirmed_force = if alias_existed && !force {
            let existing = self.config.get_alias(&name).unwrap();
            println!("{}Alias '{}' already exists:{}", COLOR_YELLOW, name, COLOR_RESET);
            println!("  {}Current:{} {}", COLOR_CYAN, COLOR_RESET, existing.command);
            if let Some(desc) = &existing.description {
                println!("  {}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }
            println!("  {}New:{} {}", COLOR_CYAN, COLOR_RESET, command);
            
            if !Self::confirm_overwrite()? {
                println!("{}Alias not modified.{}", COLOR_GRAY, COLOR_RESET);
                return Ok(());
            }
            true // User confirmed, so force the update
        } else {
            force // Use the original force value
        };

        match self.config.add_alias(name.clone(), command, description, confirmed_force) {
            Ok(true) => {
                self.save_config()?;
                if alias_existed {
                    println!("{}Updated alias '{}'{}",COLOR_GREEN, name, COLOR_RESET);
                } else {
                    println!("{}Added alias '{}'{}",COLOR_GREEN, name, COLOR_RESET);
                }
                Ok(())
            }
            Ok(false) => {
                // This shouldn't happen with the current logic, but handle it gracefully
                Err("Unexpected confirmation state".to_string())
            }
            Err(e) => Err(e)
        }
    }

    fn confirm_overwrite() -> Result<bool, String> {
        print!("{}Overwrite? (y/N):{} ", COLOR_YELLOW, COLOR_RESET);
        io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)
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
                println!("{}No aliases found matching filter.{}", COLOR_YELLOW, COLOR_RESET);
            } else {
                println!("{}No aliases configured.{}", COLOR_YELLOW, COLOR_RESET);
            }
            return;
        }

        println!("{}{}Configured aliases:{}", COLOR_BOLD, COLOR_CYAN, COLOR_RESET);
        
        // Calculate the maximum alias name length for alignment
        let max_name_len = aliases.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        let name_width = std::cmp::max(16, ((max_name_len + 4) / 4) * 4); // Minimum 16 chars, rounded to 4
        
        for (name, entry) in aliases {
            let padding = name_width.saturating_sub(name.len());
            let spaces = " ".repeat(padding);
            
            print!("  {}{}{}{} -> {}{}{}", 
                COLOR_GREEN, name, COLOR_RESET, spaces,
                COLOR_BLUE, entry.command, COLOR_RESET);
            
            if let Some(desc) = &entry.description {
                print!(" {}({}){}", COLOR_GRAY, desc, COLOR_RESET);
            }
            
            println!(" {}[{}]{}", COLOR_GRAY, entry.created, COLOR_RESET);
        }
    }

    fn which_alias(&self, name: &str) {
        if let Some(entry) = self.config.get_alias(name) {
            println!("{}Alias '{}' executes:{} {}", COLOR_CYAN, name, COLOR_RESET, entry.command);
            if let Some(desc) = &entry.description {
                println!("{}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }
        } else {
            println!("{}Alias '{}' not found.{}", COLOR_YELLOW, name, COLOR_RESET);
        }
    }

    fn show_config_location(&self) {
        println!("{}Config file location:{} {}", COLOR_CYAN, COLOR_RESET, self.config_path.display());
    }

    fn execute_alias(&self, name: &str, args: &[String]) -> Result<(), String> {
        let entry = self.config.get_alias(name)
            .ok_or_else(|| format!("Alias '{}' not found", name))?;

        let mut command_parts: Vec<&str> = entry.command.split_whitespace().collect();
        
        if command_parts.is_empty() {
            return Err("Empty command in alias".to_string());
        }

        let program = command_parts.remove(0);
        command_parts.extend(args.iter().map(|s| s.as_str()));

        let mut cmd = Command::new(program);
        cmd.args(&command_parts);
        
        // Inherit stdio so the command runs interactively
        cmd.stdin(Stdio::inherit())
           .stdout(Stdio::inherit())
           .stderr(Stdio::inherit());

        let status = cmd.status()
            .map_err(|e| format!("Failed to execute command '{}': {}", program, e))?;

        if !status.success() {
            if let Some(code) = status.code() {
                std::process::exit(code);
            } else {
                std::process::exit(1);
            }
        }

        Ok(())
    }
}

fn print_help() {
    println!("{}{}Alias Manager v{} - Cross-platform command alias tool{}", 
             COLOR_BOLD, COLOR_CYAN, VERSION, COLOR_RESET);
    println!();
    println!("{}USAGE:{}", COLOR_BOLD, COLOR_RESET);
    println!("  a [alias_name] [args...]     Execute an alias");
    println!("  a --add <n> <command> [--desc \"description\"] [--force] [--chain <command2>]");
    println!("  a --list [filter]            List aliases (optionally filtered)");
    println!("  a --remove <n>               Remove an alias");
    println!("  a --which <n>                Show what an alias does");
    println!("  a --config                   Show config file location");
    println!("  a --version                  Show version information");
    println!("  a --help                     Show this help");
    println!();
    println!("{}OPTIONS:{}", COLOR_BOLD, COLOR_RESET);
    println!("  --force                      Overwrite existing alias without confirmation");
    println!("  --chain <command>            Chain another command with && (can be used multiple times)");
    println!();
    println!("{}EXAMPLES:{}", COLOR_BOLD, COLOR_RESET);
    println!("  a --add gst \"git status\" --desc \"Quick git status\"");
    println!("  a --add deploy \"docker-compose up -d\" --force");
    println!("  a --add build \"npm run build\" --chain \"npm test\" --chain \"npm run deploy\"");
    println!("  a --list git                 # List aliases containing 'git'");
    println!("  a gst                        # Execute the 'gst' alias");
    println!("  a deploy --build             # Execute with additional args");
}

fn print_version() {
    println!("{}{}Alias Manager v{}{}", COLOR_BOLD, COLOR_CYAN, VERSION, COLOR_RESET);
    println!("A cross-platform command alias management tool written in Rust");
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
            eprintln!("{}Error initializing alias manager:{} {}", COLOR_YELLOW, COLOR_RESET, e);
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
        
        "--add" => {
            if args.len() < 4 {
                eprintln!("{}Usage:{} a --add <n> <command> [--desc \"description\"] [--force] [--chain <command2>]", COLOR_YELLOW, COLOR_RESET);
                std::process::exit(1);
            }
            
            let name = args[2].clone();
            let mut command = args[3].clone();
            
            let mut description = None;
            let mut force = false;
            let mut chained_commands = Vec::new();
            let mut i = 4;
            
            while i < args.len() {
                match args[i].as_str() {
                    "--desc" => {
                        if i + 1 < args.len() {
                            description = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("{}Error:{} --desc requires a description", COLOR_YELLOW, COLOR_RESET);
                            std::process::exit(1);
                        }
                    }
                    "--force" => {
                        force = true;
                        i += 1;
                    }
                    "--chain" => {
                        if i + 1 < args.len() {
                            chained_commands.push(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("{}Error:{} --chain requires a command", COLOR_YELLOW, COLOR_RESET);
                            std::process::exit(1);
                        }
                    }
                    _ => {
                        eprintln!("{}Error:{} Unknown option '{}'", COLOR_YELLOW, COLOR_RESET, args[i]);
                        std::process::exit(1);
                    }
                }
            }
            
            // Concatenate chained commands with &&
            for chained_cmd in chained_commands {
                command = format!("{} && {}", command, chained_cmd);
            }
            
            match manager.add_alias(name.clone(), command, description, force) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{}Error adding alias:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    std::process::exit(1);
                }
            }
        }
        
        "--list" => {
            let filter = if args.len() > 2 { Some(args[2].as_str()) } else { None };
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
                    eprintln!("{}Error executing alias:{} {}", COLOR_YELLOW, COLOR_RESET, e);
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
            "git status".to_string(),
            Some("Quick status".to_string()),
            false
        );
        
        assert!(result.is_ok());
        assert_eq!(config.aliases.len(), 1);
        
        let entry = config.get_alias("gst").unwrap();
        assert_eq!(entry.command, "git status");
        assert_eq!(entry.description, Some("Quick status".to_string()));
    }

    #[test]
    fn test_add_alias_reserved_names() {
        let mut config = Config::new();
        
        let invalid_names = vec!["--add", "mgr:test", ".hidden"];
        
        for name in invalid_names {
            let result = config.add_alias(
                name.to_string(),
                "test command".to_string(),
                None,
                false
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_remove_alias() {
        let mut config = Config::new();
        
        config.add_alias("test".to_string(), "echo test".to_string(), None, false).unwrap();
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
        config.add_alias("test".to_string(), "echo test".to_string(), None, false).unwrap();
        
        let entry = config.get_alias("test");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().command, "echo test");
        
        let entry = config.get_alias("nonexistent");
        assert!(entry.is_none());
    }

    #[test]
    fn test_list_aliases() {
        let mut config = Config::new();
        
        config.add_alias("gst".to_string(), "git status".to_string(), None, false).unwrap();
        config.add_alias("glog".to_string(), "git log".to_string(), None, false).unwrap();
        config.add_alias("deploy".to_string(), "docker-compose up".to_string(), None, false).unwrap();
        
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
        
        manager.add_alias(
            "test".to_string(),
            "echo hello".to_string(),
            Some("Test command".to_string()),
            false
        ).unwrap();
        
        // Load a new manager from the saved config
        let loaded_manager = AliasManager {
            config: AliasManager::load_config(&manager.config_path).unwrap(),
            config_path: manager.config_path.clone(),
        };
        
        let entry = loaded_manager.config.get_alias("test").unwrap();
        assert_eq!(entry.command, "echo hello");
        assert_eq!(entry.description, Some("Test command".to_string()));
    }

    #[test]
    fn test_manager_add_remove() {
        let (mut manager, _temp_dir) = create_test_manager();
        
        assert!(manager.add_alias("test".to_string(), "echo test".to_string(), None, false).is_ok());
        assert!(manager.config.get_alias("test").is_some());
        
        assert!(manager.remove_alias("test").is_ok());
        assert!(manager.config.get_alias("test").is_none());
        
        assert!(manager.remove_alias("nonexistent").is_err());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut config = Config::new();
        config.add_alias("test".to_string(), "echo test".to_string(), Some("Test".to_string()), false).unwrap();
        
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        
        let entry = deserialized.get_alias("test").unwrap();
        assert_eq!(entry.command, "echo test");
        assert_eq!(entry.description, Some("Test".to_string()));
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
}
