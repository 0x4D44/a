use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;

fn command_with_home() -> (Command, TempDir) {
    let temp_home = TempDir::new().expect("create temp home");
    let mut cmd = Command::cargo_bin("a").expect("binary exists");
    cmd.env("HOME", temp_home.path());
    cmd.env("USERPROFILE", temp_home.path());
    (cmd, temp_home)
}

fn alias_config_path(home: &TempDir) -> PathBuf {
    let config_dir = home.path().join(".alias-mgr");
    fs::create_dir_all(&config_dir).expect("create config directory");
    config_dir.join("config.json")
}

fn version_fragment() -> String {
    format!("Alias Manager v{}", env!("CARGO_PKG_VERSION"))
}

#[test]
fn no_args_shows_primary_help() {
    Command::cargo_bin("a")
        .expect("binary exists")
        .assert()
        .success()
        .stdout(predicate::str::contains(version_fragment()));
}

#[test]
fn help_with_examples_outputs_examples_section() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home); // ensure manager can initialise directories

    cmd.args(["--help", "--examples"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parallel execution"));
}

#[test]
fn version_flag_prints_version_banner() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(version_fragment()));
}

#[test]
fn config_flag_prints_config_path() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    cmd.arg("--config")
        .assert()
        .success()
        .stdout(predicate::str::contains(config_path.display().to_string()));
}

#[test]
fn which_alias_displays_alias_details() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"
{
  "aliases": {
    "demo": {
      "command_type": { "Simple": "echo hello" },
      "description": "Sample alias",
      "created": "2025-10-20"
    }
  }
}
"#;
    fs::write(&config_path, config).expect("write config file");

    cmd.args(["--which", "demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Alias 'demo' executes"))
        .stdout(predicate::str::contains("Sample alias"));
}

#[test]
fn which_alias_shows_chain_examples_and_breakdown() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"
{
  "aliases": {
    "deploy": {
      "command_type": {
        "Chain": {
          "commands": [
            {
              "command": "npm run build $1",
              "operator": null
            },
            {
              "command": "npm run test",
              "operator": "And"
            },
            {
              "command": "npm run deploy $1",
              "operator": "Always"
            }
          ],
          "parallel": true
        }
      },
      "description": "Deployment chain",
      "created": "2025-10-20"
    }
  }
}
"#;
    fs::write(&config_path, config).expect("write config");

    cmd.args(["--which", "deploy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parameter substitution example"))
        .stdout(predicate::str::contains("Command breakdown"))
        .stdout(predicate::str::contains("Execution mode:"))
        .stdout(predicate::str::contains("Parallel"));
}

#[test]
fn unknown_flag_returns_error() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--help", "--bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown option for --help"));
}

#[test]
fn missing_home_environment_reports_error() {
    let mut cmd = Command::cargo_bin("a").expect("binary exists");
    // Remove HOME and USERPROFILE to trigger configuration bootstrap failure
    cmd.env_remove("HOME");
    cmd.env_remove("USERPROFILE");
    cmd.arg("--config")
        .assert()
        .failure()
        .stderr(predicate::str::contains("environment variable not found"));
}

#[test]
fn list_aliases_shows_formatted_entries() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"
{
  "aliases": {
    "deploy": {
      "command_type": { "Simple": "npm run deploy" },
      "description": "Deploy to production",
      "created": "2025-10-20"
    },
    "test": {
      "command_type": { "Simple": "npm test" },
      "description": null,
      "created": "2025-10-20"
    }
  }
}
"#;
    fs::write(&config_path, config).expect("write config");

    cmd.arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured aliases"))
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains("Deploy to production"));
}

#[test]
fn list_aliases_with_filter_reports_empty_state() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"
{
  "aliases": {
    "deploy": {
      "command_type": { "Simple": "npm run deploy" },
      "description": null,
      "created": "2025-10-20"
    }
  }
}
"#;
    fs::write(&config_path, config).expect("write config");

    cmd.args(["--list", "missing"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No aliases found matching filter.",
        ));
}

#[test]
fn push_without_token_exits_with_error() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);
    fs::write(&config_path, r#"{"aliases":{}}"#).expect("write config");

    cmd.arg("--push")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Missing GitHub token"));
}

#[test]
fn push_with_message_parses_arguments() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);
    fs::write(&config_path, r#"{"aliases":{}}"#).expect("write config");

    cmd.args(["--push", "--message", "hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Missing GitHub token"));
}

#[test]
fn pull_with_extra_argument_is_rejected() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--pull", "extra"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--pull does not accept options"));
}

#[test]
fn export_without_config_reports_error() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);
    if config_path.exists() {
        fs::remove_file(&config_path).expect("remove config");
    }

    cmd.arg("--export")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Source config file does not exist",
        ));
}

// Phase 1: Additional CLI tests for main function coverage

#[test]
fn add_alias_success() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "myalias", "echo hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added alias 'myalias'"));
}

#[test]
fn add_alias_with_description() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args([
        "--add",
        "myalias",
        "echo test",
        "--desc",
        "Test description",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_alias_missing_arguments() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "myalias"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn add_alias_missing_name_only() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.arg("--add")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn add_alias_with_reserved_name_double_dash() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "--test", "echo hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("reserved"));
}

#[test]
fn add_alias_with_reserved_name_mgr_prefix() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "mgr:test", "echo hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("reserved"));
}

#[test]
fn add_alias_with_reserved_name_dot_prefix() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", ".hidden", "echo hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("reserved"));
}

#[test]
fn add_chain_with_and_operator() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--and", "echo two"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_chain_with_or_operator() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--or", "echo two"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_chain_with_always_operator() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--always", "echo two"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_chain_with_if_code_operator() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--if-code", "0", "echo two"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_chain_with_parallel_flag() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args([
        "--add",
        "test",
        "echo one",
        "--and",
        "echo two",
        "--parallel",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("Added alias"));
}

#[test]
fn add_chain_if_code_missing_value() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--if-code"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires an exit code"));
}

#[test]
fn add_chain_if_code_invalid_value() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--if-code", "abc", "echo two"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("numeric exit code"));
}

#[test]
fn add_chain_operator_missing_command() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--add", "test", "echo one", "--and"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a command"));
}

#[test]
fn remove_alias_success() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    // First create an alias
    let config = r#"{"aliases":{"test":{"command_type":{"Simple":"echo hello"},"description":null,"created":"2025-10-20"}}}"#;
    fs::write(&config_path, config).expect("write config");

    cmd.args(["--remove", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed alias 'test'"));
}

#[test]
fn remove_alias_not_found() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--remove", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Alias 'nonexistent' not found"));
}

#[test]
fn remove_alias_missing_name() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.arg("--remove")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn list_with_matching_filter() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"{
  "aliases": {
    "deploy": {
      "command_type": { "Simple": "npm run deploy" },
      "description": null,
      "created": "2025-10-20"
    },
    "test": {
      "command_type": { "Simple": "npm test" },
      "description": null,
      "created": "2025-10-20"
    }
  }
}"#;
    fs::write(&config_path, config).expect("write config");

    cmd.args(["--list", "dep"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains("test").not());
}

#[test]
fn which_alias_not_found() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--which", "nonexistent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Alias 'nonexistent' not found"));
}

#[test]
fn which_alias_missing_name() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.arg("--which")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn execute_alias_simple() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"{
  "aliases": {
    "greet": {
      "command_type": { "Simple": "echo hello" },
      "description": null,
      "created": "2025-10-20"
    }
  }
}"#;
    fs::write(&config_path, config).expect("write config");

    cmd.arg("greet").assert().success();
}

#[test]
fn execute_alias_with_arguments() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"{
  "aliases": {
    "greet": {
      "command_type": { "Simple": "echo" },
      "description": null,
      "created": "2025-10-20"
    }
  }
}"#;
    fs::write(&config_path, config).expect("write config");

    cmd.args(["greet", "hello", "world"]).assert().success();
}

#[test]
fn execute_alias_not_found() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Alias 'nonexistent' not found"));
}

// Note: --import command doesn't exist, removed these tests

#[test]
fn export_config_success() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);

    let config = r#"{"aliases":{"test":{"command_type":{"Simple":"echo test"},"description":null,"created":"2025-10-20"}}}"#;
    fs::write(&config_path, config).expect("write config");

    let export_dir = home.path().join("export_test");
    cmd.args(["--export", export_dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("exported"));

    assert!(export_dir.join("config.json").exists());
}

#[test]
fn help_with_invalid_option() {
    let (mut cmd, home) = command_with_home();
    let _ = alias_config_path(&home);

    cmd.args(["--help", "--invalid"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown option"));
}

#[test]
fn push_with_multiple_message_parts() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);
    fs::write(&config_path, r#"{"aliases":{}}"#).expect("write config");

    // This will fail because "config" is treated as an extra argument
    cmd.args(["--push", "--message", "Update", "config", "file"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown or unsupported option"));
}

#[test]
fn push_with_message_missing_value() {
    let (mut cmd, home) = command_with_home();
    let config_path = alias_config_path(&home);
    fs::write(&config_path, r#"{"aliases":{}}"#).expect("write config");

    // When --message has no value, it's treated as an unknown option
    cmd.args(["--push", "--message"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown or unsupported option"));
}
