use crate::{PROJECT_DESCRIPTION, PROJECT_NAME, VERSION};

const DOMAINS: &[&str] = &[
    "system", "service", "process", "boot", "target", "mount", "log", "watchdog",
    "config", "dependency", "command",
];

const HELP: &str = r#"Frenit — Free Init

Usage:
  frenit <domain> <action> [argument]
  frenit help [domain]
  frenit --version

Domains:
  system       System status, diagnostics, and power management
  service      Service lifecycle and configuration
  process      Process inspection and control
  boot         Boot status and boot targets
  target       Operating targets
  mount        Filesystem mount management
  log          Boot, service, and kernel logs
  watchdog     Watchdog status
  config       Configuration management
  dependency   Service dependencies and execution order
  command      Command vocabulary

Examples:
  frenit system status
  frenit service start network
  frenit process show 1234
  frenit dependency graph
  frenit system poweroff

Runtime operations are not yet connected to the Wicofe kernel or service manager.
"#;

pub fn run<I>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();

    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        return Ok(HELP.to_owned());
    }
    if args[0] == "help" {
        if args.len() > 2 {
            return Err("usage: frenit help [domain]".to_owned());
        }
        return help_for(args.get(1).map(String::as_str));
    }
    if args[0] == "--version" || args[0] == "-V" {
        if args.len() != 1 {
            return Err("usage: frenit --version".to_owned());
        }
        return Ok(format!("{PROJECT_NAME} ({PROJECT_DESCRIPTION}) {VERSION}"));
    }
    if args.as_slice() == ["system", "version"] {
        return Ok(format!("{PROJECT_NAME} ({PROJECT_DESCRIPTION}) {VERSION}"));
    }
    if args.as_slice() == ["command", "list"] {
        return Ok(DOMAINS.join("\n"));
    }

    validate_command(&args)?;
    Err(format!(
        "'{}' is recognized, but its runtime backend is not implemented yet",
        args.join(" ")
    ))
}

fn help_for(domain: Option<&str>) -> Result<String, String> {
    match domain {
        None => Ok(HELP.to_owned()),
        Some("system") => Ok("system actions: status, version, info, health, diagnostic, recovery, shutdown, reboot, poweroff, halt, suspend".to_owned()),
        Some("service") => Ok("service actions: start, stop, restart, reload, enable, disable, list, show, mask, unmask".to_owned()),
        Some("process") => Ok("process actions: list, show, kill, signal, terminate, tree".to_owned()),
        Some("boot") => Ok("boot actions: status, log, list, target, default".to_owned()),
        Some("target") => Ok("target actions: list, get, set".to_owned()),
        Some("mount") => Ok("mount actions: list, status, all, mount, umount, remount".to_owned()),
        Some("log") => Ok("log actions: boot, service, kernel, clear".to_owned()),
        Some("watchdog") => Ok("watchdog actions: status".to_owned()),
        Some("config") => Ok("config actions: show, validate, reload, test".to_owned()),
        Some("dependency") => Ok("dependency actions: deps, dependent, graph, order".to_owned()),
        Some("command") => Ok("command actions: list".to_owned()),
        Some(other) => Err(format!("unknown domain '{other}'. Run 'frenit help'.")),
    }
}

fn validate_command(args: &[String]) -> Result<(), String> {
    let domain = args.first().map(String::as_str).unwrap_or_default();
    let action = args.get(1).map(String::as_str).unwrap_or_default();

    if !DOMAINS.contains(&domain) {
        return Err(format!("unknown domain '{domain}'. Run 'frenit help'."));
    }

    let valid = match (domain, action) {
        (
            "system",
            "status" | "version" | "info" | "health" | "diagnostic" | "recovery" | "shutdown"
            | "reboot" | "poweroff" | "halt" | "suspend",
        ) => args.len() == 2,
        ("service", "list") => args.len() == 2,
        (
            "service",
            "start" | "stop" | "restart" | "reload" | "enable" | "disable" | "show" | "mask"
            | "unmask",
        ) => args.len() == 3,
        ("process", "list" | "tree") => args.len() == 2,
        ("process", "show" | "kill" | "terminate") => args.len() == 3,
        ("process", "signal") => args.len() == 4,
        ("boot", "status" | "log" | "list" | "target" | "default") => args.len() == 2,
        ("target", "list" | "get") => args.len() == 2,
        ("target", "set") => args.len() == 3,
        ("mount", "list" | "status" | "all") => args.len() == 2,
        ("mount", "umount" | "remount") => args.len() == 3,
        ("mount", "mount") => args.len() == 4,
        ("log", "boot" | "kernel" | "clear") => args.len() == 2,
        ("log", "service") => args.len() == 3,
        ("watchdog", "status") => args.len() == 2,
        ("config", "show" | "validate" | "reload" | "test") => args.len() == 2,
        ("dependency", "graph" | "order") => args.len() == 2,
        ("dependency", "deps" | "dependent") => args.len() == 3,
        ("command", "list") => args.len() == 2,
        _ => false,
    };

    if !valid {
        return Err(format!(
            "invalid action or arguments for '{domain}'. Run 'frenit help {domain}'."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(items: &[&str]) -> Result<String, String> {
        run(items.iter().map(|s| (*s).to_owned()))
    }

    fn validate(items: &[&str]) -> Result<(), String> {
        validate_command(&items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn help_is_available_without_arguments() {
        assert!(run_args(&[]).unwrap().contains("Frenit"));
        assert!(run_args(&["--help"]).unwrap().contains("frenit <domain>"));
    }

    #[test]
    fn version_is_available_from_root_and_system_domain() {
        assert!(run_args(&["--version"]).unwrap().contains("Frenit"));
        assert!(run_args(&["system", "version"]).unwrap().contains("Frenit"));
        assert!(run_args(&["--version", "extra"]).is_err());
    }

    #[test]
    fn command_lists_domains() {
        let output = run_args(&["command", "list"]).unwrap();
        assert!(output.lines().any(|line| line == "dependency"));
        assert!(output.lines().any(|line| line == "system"));
    }

    #[test]
    fn rejects_flat_legacy_commands() {
        assert!(validate(&["status"]).is_err());
        assert!(validate(&["diagnostic"]).is_err());
        assert!(validate(&["deps", "network"]).is_err());
    }

    #[test]
    fn validates_service_action_and_argument() {
        assert!(validate(&["service", "start", "sshd"]).is_ok());
        assert!(validate(&["service", "start"]).is_err());
        assert!(validate(&["service", "explode"]).is_err());
        assert!(validate(&["service", "list", "extra"]).is_err());
    }

    #[test]
    fn validates_process_action_shapes() {
        assert!(validate(&["process", "signal", "1234", "TERM"]).is_ok());
        assert!(validate(&["process", "signal", "1234"]).is_err());
        assert!(validate(&["process", "show", "1234"]).is_ok());
        assert!(validate(&["process", "tree", "extra"]).is_err());
    }

    #[test]
    fn dependency_actions_require_a_service_when_needed() {
        assert!(validate(&["dependency", "deps", "network"]).is_ok());
        assert!(validate(&["dependency", "dependent", "network"]).is_ok());
        assert!(validate(&["dependency", "dependent"]).is_err());
        assert!(validate(&["dependency", "graph"]).is_ok());
    }

    #[test]
    fn power_actions_have_distinct_commands() {
        assert!(validate(&["system", "shutdown"]).is_ok());
        assert!(validate(&["system", "poweroff"]).is_ok());
        assert!(validate(&["system", "reboot"]).is_ok());
        assert!(validate(&["system", "shutdown", "now"]).is_err());
    }

    #[test]
    fn domain_help_is_available_and_rejects_extra_arguments() {
        assert!(run_args(&["help", "service"]).unwrap().contains("restart"));
        assert!(run_args(&["help", "service", "extra"]).is_err());
        assert!(run_args(&["help", "unknown"]).is_err());
    }
}
