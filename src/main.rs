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
enum ChainOperator {
    And,        // && - run if previous succeeded
    Or,         // || - run if previous failed  
    Always,     // ; - always run regardless
    IfCode(i32), // run if previous exit code equals N
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChainCommand {
    command: String,
    operator: Option<ChainOperator>,  // None for the first command
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CommandChain {
    commands: Vec<ChainCommand>,
    parallel: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum CommandType {
    Simple(String),           // Single command (backward compatibility)
    Chain(CommandChain),      // Complex command chain
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

    fn add_alias(&mut self, name: String, command_type: CommandType, description: Option<String>, force: bool) -> Result<bool, String> {
        if name.starts_with("--") || name.contains("mgr:") || name.starts_with(".") {
            return Err(format!("Invalid alias name '{}': cannot use reserved prefixes", name));
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

    fn add_alias(&mut self, name: String, command_type: CommandType, description: Option<String>, force: bool) -> Result<(), String> {
        // Check if alias already exists before making changes
        let alias_existed = self.config.aliases.contains_key(&name);
        
        // Check if alias exists and get confirmation if needed
        let confirmed_force = if alias_existed && !force {
            let existing = self.config.get_alias(&name).unwrap();
            println!("{}Alias '{}' already exists:{}", COLOR_YELLOW, name, COLOR_RESET);
            println!("  {}Current:{} {}", COLOR_CYAN, COLOR_RESET, existing.command_display());
            if let Some(desc) = &existing.description {
                println!("  {}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }
            println!("  {}New:{} {}", COLOR_CYAN, COLOR_RESET, 
                match &command_type {
                    CommandType::Simple(cmd) => cmd.clone(),
                    CommandType::Chain(chain) => format!("Complex chain with {} commands", chain.commands.len()),
                });
            
            if !Self::confirm_overwrite()? {
                println!("{}Alias not modified.{}", COLOR_GRAY, COLOR_RESET);
                return Ok(());
            }
            true // User confirmed, so force the update
        } else {
            force // Use the original force value
        };

        match self.config.add_alias(name.clone(), command_type, description, confirmed_force) {
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
                COLOR_BLUE, entry.command_display(), COLOR_RESET);
            
            if let Some(desc) = &entry.description {
                print!(" {}({}){}", COLOR_GRAY, desc, COLOR_RESET);
            }
            
            println!(" {}[{}]{}", COLOR_GRAY, entry.created, COLOR_RESET);
        }
    }

    fn which_alias(&self, name: &str) {
        if let Some(entry) = self.config.get_alias(name) {
            println!("{}Alias '{}' executes:{} {}", COLOR_CYAN, name, COLOR_RESET, entry.command_display());
            if let Some(desc) = &entry.description {
                println!("{}Description:{} {}", COLOR_CYAN, COLOR_RESET, desc);
            }
            
            // Show detailed breakdown for complex chains
            if let CommandType::Chain(chain) = &entry.command_type {
                println!("{}Command breakdown:{}", COLOR_CYAN, COLOR_RESET);
                for (i, chain_cmd) in chain.commands.iter().enumerate() {
                    let op_desc = match &chain_cmd.operator {
                        Some(ChainOperator::And) => " (run if previous succeeded)",
                        Some(ChainOperator::Or) => " (run if previous failed)",
                        Some(ChainOperator::Always) => " (always run)",
                        Some(ChainOperator::IfCode(code)) => &format!(" (run if previous exit code = {})", code),
                        None => "",
                    };
                    println!("  {}{}. {}{}{}{}", COLOR_GRAY, i + 1, COLOR_RESET, chain_cmd.command, COLOR_GRAY, op_desc);
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
        println!("{}Config file location:{} {}", COLOR_CYAN, COLOR_RESET, self.config_path.display());
    }

    fn execute_alias(&self, name: &str, args: &[String]) -> Result<(), String> {
        let entry = self.config.get_alias(name)
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

    fn execute_legacy_command_chain(&self, full_command: &str, additional_args: &[String]) -> Result<(), String> {
        let commands: Vec<&str> = full_command.split(" && ").collect();
        
        for (index, command_str) in commands.iter().enumerate() {
            let command_str = command_str.trim();
            if command_str.is_empty() {
                continue;
            }

            // Only add additional args to the last command in the chain
            let args_to_use = if index == commands.len() - 1 { additional_args } else { &[] };
            
            println!("{}[{}/{}]{} Executing: {}{}{}", 
                     COLOR_GRAY, index + 1, commands.len(), COLOR_RESET,
                     COLOR_CYAN, command_str, COLOR_RESET);
            
            match self.execute_single_command(command_str, args_to_use) {
                Ok(()) => continue,
                Err(e) => {
                    eprintln!("{}Command failed:{} {}", COLOR_YELLOW, COLOR_RESET, e);
                    eprintln!("{}Stopping command chain at step {}/{}{}", 
                             COLOR_YELLOW, index + 1, commands.len(), COLOR_RESET);
                    return Err(format!("Command chain stopped at step {}", index + 1));
                }
            }
        }
        
        println!("{}Command chain completed successfully{}", COLOR_GREEN, COLOR_RESET);
        Ok(())
    }

    fn execute_sequential_chain(&self, chain: &CommandChain, additional_args: &[String]) -> Result<(), String> {
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
                    Some(ChainOperator::And) => format!("previous command failed (exit code {})", last_exit_code),
                    Some(ChainOperator::Or) => "previous command succeeded".to_string(),
                    Some(ChainOperator::IfCode(code)) => format!("previous exit code was {}, expected {}", last_exit_code, code),
                    _ => "unknown condition".to_string(),
                };
                println!("{}[{}/{}]{} Skipping: {}{}{} ({})", 
                         COLOR_GRAY, index + 1, chain.commands.len(), COLOR_RESET,
                         COLOR_GRAY, chain_cmd.command, COLOR_RESET, reason);
                continue;
            }

            // Only add additional args to the last command in the chain
            let args_to_use = if index == chain.commands.len() - 1 { additional_args } else { &[] };
            
            let op_desc = match &chain_cmd.operator {
                Some(ChainOperator::And) => " (&&)",
                Some(ChainOperator::Or) => " (||)",
                Some(ChainOperator::Always) => " (;)",
                Some(ChainOperator::IfCode(code)) => &format!(" (?[{}])", code),
                None => "",
            };
            
            println!("{}[{}/{}]{}{} Executing: {}{}{}", 
                     COLOR_GRAY, index + 1, chain.commands.len(), COLOR_RESET, op_desc,
                     COLOR_CYAN, chain_cmd.command, COLOR_RESET);
            
            last_exit_code = match self.execute_single_command_with_exit_code(&chain_cmd.command, args_to_use) {
                Ok(code) => code,
                Err(_) => {
                    // Command failed to execute (e.g., program not found)
                    // Treat this as exit code 127 (command not found) and continue
                    127
                }
            };
        }
        
        println!("{}Sequential command chain completed{}", COLOR_GREEN, COLOR_RESET);
        Ok(())
    }

    fn execute_parallel_chain(&self, chain: &CommandChain, additional_args: &[String]) -> Result<(), String> {
        use std::thread;
        use std::sync::mpsc;
        
        println!("{}Executing {} commands in parallel{}", COLOR_CYAN, chain.commands.len(), COLOR_RESET);
        
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        
        for (index, chain_cmd) in chain.commands.iter().enumerate() {
            let cmd = chain_cmd.command.clone();
            let cmd_display = cmd.clone(); // Clone for display purposes
            let args = if index == chain.commands.len() - 1 { 
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
            println!("{}Started:{} {}{}{}", COLOR_GRAY, COLOR_RESET, COLOR_CYAN, cmd_display, COLOR_RESET);
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
                        println!("{}Completed [{}]:{} exit code {}", COLOR_GREEN, index + 1, COLOR_RESET, code);
                    } else {
                        let error = results.last().unwrap().1.as_ref().err().unwrap();
                        println!("{}Failed [{}]:{} {}", COLOR_YELLOW, index + 1, COLOR_RESET, error);
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
        let failed_commands: Vec<_> = results.iter()
            .filter(|(_, result)| result.is_err())
            .collect();
            
        if failed_commands.is_empty() {
            println!("{}All parallel commands completed successfully{}", COLOR_GREEN, COLOR_RESET);
            Ok(())
        } else {
            eprintln!("{}Failed commands: {}/{}{}", 
                     COLOR_YELLOW, failed_commands.len(), chain.commands.len(), COLOR_RESET);
            Err(format!("{} parallel commands failed", failed_commands.len()))
        }
    }

    fn execute_single_command_with_exit_code(&self, command_str: &str, args: &[String]) -> Result<i32, String> {
        let mut command_parts: Vec<&str> = command_str.split_whitespace().collect();
        
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

        Ok(status.code().unwrap_or(1))
    }

    fn execute_command_static(command_str: &str, args: &[String]) -> Result<i32, String> {
        let mut command_parts: Vec<&str> = command_str.split_whitespace().collect();
        
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

        Ok(status.code().unwrap_or(1))
    }

    fn execute_single_command(&self, command_str: &str, args: &[String]) -> Result<(), String> {
        let mut command_parts: Vec<&str> = command_str.split_whitespace().collect();
        
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
            let exit_code = status.code().unwrap_or(1);
            if exit_code != 0 {
                std::process::exit(exit_code);
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
    println!("  a --add <n> <command> [OPTIONS]");
    println!("  a --list [filter]            List aliases (optionally filtered)");
    println!("  a --remove <n>               Remove an alias");
    println!("  a --which <n>                Show what an alias does");
    println!("  a --config                   Show config file location");
    println!("  a --version                  Show version information");
    println!("  a --help                     Show this help");
    println!();
    println!("{}ADD OPTIONS:{}", COLOR_BOLD, COLOR_RESET);
    println!("  --desc \"description\"        Add a description");
    println!("  --force                      Overwrite existing alias without confirmation");
    println!("  --chain <command>            Legacy: Chain with && (same as --and)");
    println!("  --and <command>              Chain command (run if previous succeeded)");
    println!("  --or <command>               Chain command (run if previous failed)");
    println!("  --always <command>           Chain command (always run regardless)");
    println!("  --if-code <N> <command>      Chain command (run if previous exit code = N)");
    println!("  --parallel                   Execute all commands in parallel");
    println!();
    println!("{}EXAMPLES:{}", COLOR_BOLD, COLOR_RESET);
    println!("  # Simple alias");
    println!("  a --add gst \"git status\" --desc \"Quick git status\"");
    println!();
    println!("  # Sequential execution (default)");
    println!("  a --add deploy \"npm run build\" --and \"npm test\" --and \"npm run deploy\"");
    println!();
    println!("  # Complex conditional logic");
    println!("  a --add smart \"npm test\" --and \"npm run deploy\" --or \"echo 'Tests failed!'\"");
    println!();
    println!("  # Exit code handling");
    println!("  a --add check \"npm test\" --if-code 0 \"echo 'All good!'\" --if-code 1 \"echo 'Tests failed'\"");
    println!();
    println!("  # Parallel execution");
    println!("  a --add build \"npm run lint\" --and \"npm run test\" --parallel");
    println!();
    println!("  # Always run cleanup");
    println!("  a --add deploy \"npm run build\" --and \"npm run deploy\" --always \"npm run cleanup\"");
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
                eprintln!("{}Usage:{} a --add <n> <command> [OPTIONS]", COLOR_YELLOW, COLOR_RESET);
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
                            eprintln!("{}Error:{} --desc requires a description", COLOR_YELLOW, COLOR_RESET);
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
                            eprintln!("{}Error:{} {} requires a command", COLOR_YELLOW, COLOR_RESET, args[i]);
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
                            eprintln!("{}Error:{} --or requires a command", COLOR_YELLOW, COLOR_RESET);
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
                            eprintln!("{}Error:{} --always requires a command", COLOR_YELLOW, COLOR_RESET);
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
                                    eprintln!("{}Error:{} --if-code requires a numeric exit code", COLOR_YELLOW, COLOR_RESET);
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            eprintln!("{}Error:{} --if-code requires an exit code and a command", COLOR_YELLOW, COLOR_RESET);
                            std::process::exit(1);
                        }
                    }
                    _ => {
                        eprintln!("{}Error:{} Unknown option '{}'", COLOR_YELLOW, COLOR_RESET, args[i]);
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
            CommandType::Simple("git status".to_string()),
            Some("Quick status".to_string()),
            false
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
                false
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_remove_alias() {
        let mut config = Config::new();
        
        config.add_alias("test".to_string(), CommandType::Simple("echo test".to_string()), None, false).unwrap();
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
        config.add_alias("test".to_string(), CommandType::Simple("echo test".to_string()), None, false).unwrap();
        
        let entry = config.get_alias("test");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().command_display(), "echo test");
        
        let entry = config.get_alias("nonexistent");
        assert!(entry.is_none());
    }

    #[test]
    fn test_list_aliases() {
        let mut config = Config::new();
        
        config.add_alias("gst".to_string(), CommandType::Simple("git status".to_string()), None, false).unwrap();
        config.add_alias("glog".to_string(), CommandType::Simple("git log".to_string()), None, false).unwrap();
        config.add_alias("deploy".to_string(), CommandType::Simple("docker-compose up".to_string()), None, false).unwrap();
        
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
            CommandType::Simple("echo hello".to_string()),
            Some("Test command".to_string()),
            false
        ).unwrap();
        
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
        
        assert!(manager.add_alias("test".to_string(), CommandType::Simple("echo test".to_string()), None, false).is_ok());
        assert!(manager.config.get_alias("test").is_some());
        
        assert!(manager.remove_alias("test").is_ok());
        assert!(manager.config.get_alias("test").is_none());
        
        assert!(manager.remove_alias("nonexistent").is_err());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut config = Config::new();
        config.add_alias("test".to_string(), CommandType::Simple("echo test".to_string()), Some("Test".to_string()), false).unwrap();
        
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
                ChainCommand { command: "echo first".to_string(), operator: None },
                ChainCommand { command: "echo second".to_string(), operator: Some(ChainOperator::And) },
                ChainCommand { command: "echo third".to_string(), operator: Some(ChainOperator::Or) },
            ],
            parallel: false,
        };
        
        config.add_alias("test".to_string(), CommandType::Chain(chain), None, false).unwrap();
        
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
}
