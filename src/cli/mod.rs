use crate::{PROJECT_DESCRIPTION, PROJECT_NAME, VERSION};

const COMMANDS: &[&str] = &[
    "status", "version", "info", "help", "service", "process", "boot",
    "target", "mount", "log", "health", "diagnostic", "recovery",
    "watchdog", "config", "deps", "dependent", "graph", "order",
    "command", "system",
];

const HELP: &str = r#"Finit — Free Init

Usage:
  finit <command> [arguments]

System:
  status | version | info | help [command] | command

Service:
  service start|stop|restart|reload|enable|disable|list|show|mask|unmask

Process:
  process list|show|kill|signal|terminate|tree

Boot and targets:
  boot status|log|list|target|default
  target list|get|set

System resources:
  mount list|status|all|mount|umount|remount
  log [boot|service|kernel|clear]
  health | diagnostic | recovery | watchdog status

Configuration and dependencies:
  config show|validate|reload|test
  deps <service> | dependent <service> | graph | order

Power:
  system shutdown|reboot|poweroff|halt|suspend

This build provides the CLI foundation only. Runtime operations are not yet
connected to the Wicofe kernel or service manager.
"#;

pub fn run<I>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();

    if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
        return Ok(HELP.to_owned());
    }
    if args[0] == "--version" || args[0] == "-V" || args[0] == "version" {
        return Ok(format!("{PROJECT_NAME} ({PROJECT_DESCRIPTION}) {VERSION}"));
    }
    if args[0] == "command" {
        return Ok(COMMANDS.join("\n"));
    }

    validate_command(&args)?;
    Err(format!(
        "'{}' is recognized, but its runtime backend is not implemented yet",
        args.join(" ")
    ))
}

fn validate_command(args: &[String]) -> Result<(), String> {
    let root = args.first().map(String::as_str).unwrap_or_default();
    if !COMMANDS.contains(&root) {
        return Err(format!("unknown command '{root}'. Run 'finit help'."));
    }

    let valid = match root {
        "service" => matches_subcommand(args, &[
            "start", "stop", "restart", "reload", "enable", "disable",
            "list", "show", "mask", "unmask",
        ]),
        "process" => matches_subcommand(args, &[
            "list", "show", "kill", "signal", "terminate", "tree",
        ]),
        "boot" => matches_subcommand(args, &[
            "status", "log", "list", "target", "default",
        ]),
        "target" => matches_subcommand(args, &["list", "get", "set"]),
        "mount" => matches_subcommand(args, &[
            "list", "status", "all", "mount", "umount", "remount",
        ]),
        "config" => matches_subcommand(args, &["show", "validate", "reload", "test"]),
        "system" => matches_subcommand(args, &[
            "shutdown", "reboot", "poweroff", "halt", "suspend",
        ]),
        "watchdog" => matches_subcommand(args, &["status"]),
        "log" => args.len() <= 3 && (args.len() == 1 || matches!(
            args[1].as_str(), "boot" | "service" | "kernel" | "clear"
        )),
        "deps" | "dependent" => args.len() == 2,
        "status" | "info" | "health" | "diagnostic" | "recovery" | "graph" | "order" => args.len() == 1,
        _ => true,
    };

    if !valid {
        return Err(format!("invalid arguments for '{root}'. Run 'finit help'."));
    }
    Ok(())
}

fn matches_subcommand(args: &[String], allowed: &[&str]) -> bool {
    args.get(1).is_some_and(|sub| allowed.contains(&sub.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(items: &[&str]) -> Result<String, String> {
        run(items.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn help_is_available_without_arguments() {
        assert!(run_args(&[]).unwrap().contains("Finit"));
    }

    #[test]
    fn command_lists_root_commands() {
        let output = run_args(&["command"]).unwrap();
        assert!(output.lines().any(|line| line == "diagnostic"));
        assert!(output.lines().any(|line| line == "dependent"));
    }

    #[test]
    fn rejects_old_plural_diagnostic_command() {
        assert!(run_args(&["diagnostics"]).unwrap_err().contains("unknown command"));
    }

    #[test]
    fn validates_service_shape() {
        assert!(validate_command(&["service".into(), "start".into(), "sshd".into()]).is_ok());
        assert!(validate_command(&["service".into(), "explode".into()]).is_err());
    }

    #[test]
    fn dependency_commands_require_a_service() {
        assert!(validate_command(&["deps".into()]).is_err());
        assert!(validate_command(&["dependent".into(), "network".into()]).is_ok());
    }
}
