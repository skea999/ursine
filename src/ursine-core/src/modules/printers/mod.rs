//! printers — CUPS printers and queues.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, PrintJob, Printer};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const LPSTAT: [&str; 3] = ["/usr/bin/lpstat", "/usr/sbin/lpstat", "/bin/lpstat"];
const LPADMIN: [&str; 3] = ["/usr/sbin/lpadmin", "/usr/bin/lpadmin", "/sbin/lpadmin"];
const CUPSENABLE: [&str; 3] = [
    "/usr/sbin/cupsenable",
    "/usr/bin/cupsenable",
    "/sbin/cupsenable",
];
const CUPSDISABLE: [&str; 3] = [
    "/usr/sbin/cupsdisable",
    "/usr/bin/cupsdisable",
    "/sbin/cupsdisable",
];
const CANCEL: [&str; 3] = ["/usr/bin/cancel", "/usr/sbin/cancel", "/bin/cancel"];
const TIMEOUT: Duration = Duration::from_secs(120);

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListPrinters => Ok(json!({"printers": list().await?})),
        Action::PrinterJobs { printer } => Ok(json!({"jobs": jobs(printer.as_deref()).await?})),
        Action::AddPrinter {
            name,
            uri,
            description,
            location,
        } => {
            validate_name(name)?;
            validate_uri(uri)?;
            validate_text(description, "description")?;
            validate_text(location, "location")?;
            let mut args = vec!["-p", name, "-E", "-v", uri, "-m", "everywhere"];
            if !description.trim().is_empty() {
                args.extend(["-D", description.trim()]);
            }
            if !location.trim().is_empty() {
                args.extend(["-L", location.trim()]);
            }
            let output = run_quoted(&LPADMIN, &args).await?;
            Ok(json!({"output": output}))
        }
        Action::AddPrinterManual {
            name,
            uri,
            driver,
            ppd,
            description,
            location,
            options,
        } => {
            validate_name(name)?;
            validate_uri(uri)?;
            validate_text(description, "description")?;
            validate_text(location, "location")?;
            if !driver.is_empty() && !ppd.is_empty() {
                return Err(ApiError::invalid("choose a driver or a PPD, not both"));
            }
            if !ppd.is_empty() {
                validate_ppd(ppd)?;
            }
            let mut args = vec!["-p", name, "-E", "-v", uri];
            if ppd.is_empty() {
                let driver = if driver.is_empty() {
                    "everywhere"
                } else {
                    driver
                };
                validate_driver(driver)?;
                args.extend(["-m", driver]);
            } else {
                args.extend(["-P", ppd]);
            }
            if !description.trim().is_empty() {
                args.extend(["-D", description.trim()]);
            }
            if !location.trim().is_empty() {
                args.extend(["-L", location.trim()]);
            }
            for option in options {
                validate_option(option)?;
                args.extend(["-o", option]);
            }
            let output = run_quoted(&LPADMIN, &args).await?;
            Ok(json!({"output": output}))
        }
        Action::RemovePrinter { name } => {
            validate_name(name)?;
            Ok(json!({"output": run(&LPADMIN, &["-x", name]).await?}))
        }
        Action::SetDefaultPrinter { name } => {
            validate_name(name)?;
            Ok(json!({"output": run(&LPADMIN, &["-d", name]).await?}))
        }
        Action::SetPrinterOptions { name, options } => {
            validate_name(name)?;
            if options.is_empty() {
                return Err(ApiError::invalid("at least one printer option is required"));
            }
            let mut args = vec!["-p", name];
            for option in options {
                validate_option(option)?;
                args.extend(["-o", option]);
            }
            Ok(json!({"output": run_quoted(&LPADMIN, &args).await?}))
        }
        Action::PausePrinter { name, pause } => {
            validate_name(name)?;
            let program = if *pause { CUPSDISABLE } else { CUPSENABLE };
            Ok(json!({"output": run(&program, &[name]).await?}))
        }
        Action::CancelPrinterJobs { printer } => {
            let mut args = vec!["-a"];
            if let Some(printer) = printer {
                validate_name(printer)?;
                args.push(printer);
            }
            Ok(json!({"output": run(&CANCEL, &args).await?}))
        }
        _ => Err(ApiError::invalid("action is not valid for module printers")),
    }
}

async fn list() -> Result<Vec<Printer>, ApiError> {
    let output = run(&LPSTAT, &["-p", "-d", "-l"]).await?;
    Ok(parse_printers(&output))
}

async fn jobs(printer: Option<&str>) -> Result<Vec<PrintJob>, ApiError> {
    if let Some(printer) = printer {
        validate_name(printer)?;
    }
    let mut args = vec!["-o"];
    if let Some(printer) = printer {
        args.push(printer);
    }
    let output = run(&LPSTAT, &args).await?;
    Ok(parse_jobs(&output, printer))
}

fn parse_printers(output: &str) -> Vec<Printer> {
    let default = output.lines().find_map(|line| {
        line.strip_prefix("system default destination: ")
            .map(str::trim)
    });
    let mut printers: Vec<Printer> = Vec::new();
    let mut current: Option<usize> = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("printer ") {
            let mut fields = rest.splitn(2, ' ');
            let Some(name) = fields.next() else {
                continue;
            };
            let state = fields.next().unwrap_or_default().to_string();
            printers.push(Printer {
                name: name.to_string(),
                accepting: !state.contains("disabled"),
                state,
                is_default: default == Some(name),
                description: None,
                location: None,
                model: None,
            });
            current = Some(printers.len() - 1);
            continue;
        }
        let Some(index) = current else {
            continue;
        };
        let Some(printer) = printers.get_mut(index) else {
            continue;
        };
        if let Some(value) = line.trim().strip_prefix("Description:") {
            printer.description = Some(value.trim().to_string());
        } else if let Some(value) = line.trim().strip_prefix("Location:") {
            printer.location = Some(value.trim().to_string());
        } else if let Some(value) = line.trim().strip_prefix("Interface:") {
            printer.model = Some(value.trim().to_string());
        }
    }
    printers
}

fn parse_jobs(output: &str, printer: Option<&str>) -> Vec<PrintJob> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?.to_string();
            let user = fields.next()?.to_string();
            let size = fields.next().map(str::to_string);
            let submitted = line
                .split_once(&user)
                .map(|(_, rest)| rest.trim().to_string())
                .filter(|value| !value.is_empty());
            let name = printer
                .map(str::to_string)
                .or_else(|| id.rsplit_once('-').map(|(printer, _)| printer.to_string()))
                .unwrap_or_default();
            let state = if line.contains("processing") {
                Some("processing".to_string())
            } else if line.contains("held") {
                Some("held".to_string())
            } else {
                Some("queued".to_string())
            };
            Some(PrintJob {
                id,
                printer: name,
                user,
                size,
                submitted,
                state,
            })
        })
        .collect()
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(ApiError::invalid("invalid printer name"));
    }
    Ok(())
}

fn validate_uri(uri: &str) -> Result<(), ApiError> {
    let valid = ["ipp://", "ipps://", "usb://", "socket://", "lpd://"]
        .iter()
        .any(|prefix| uri.starts_with(prefix));
    if !valid || uri.len() > 1024 || uri.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "URI must start with ipp://, ipps://, usb://, socket:// or lpd://",
        ));
    }
    Ok(())
}

fn validate_driver(driver: &str) -> Result<(), ApiError> {
    if driver.len() > 512
        || driver.starts_with('-')
        || !driver
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+'))
    {
        return Err(ApiError::invalid("invalid printer driver"));
    }
    Ok(())
}

fn validate_ppd(ppd: &str) -> Result<(), ApiError> {
    let path = Path::new(ppd);
    if !path.is_absolute()
        || !path.is_file()
        || !(ppd.starts_with("/usr/share/ppd/") || ppd.starts_with("/etc/cups/ppd/"))
    {
        return Err(ApiError::invalid(
            "PPD must be a file under /usr/share/ppd or /etc/cups/ppd",
        ));
    }
    Ok(())
}

fn validate_option(option: &str) -> Result<(), ApiError> {
    let Some((key, value)) = option.split_once('=') else {
        return Err(ApiError::invalid("printer option must be key=value"));
    };
    if key.is_empty()
        || key.len() > 128
        || value.len() > 512
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        || value.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid("invalid printer option"));
    }
    Ok(())
}

fn validate_text(value: &str, label: &str) -> Result<(), ApiError> {
    if value.len() > 256 || value.chars().any(char::is_control) {
        return Err(ApiError::invalid(format!("invalid {label}")));
    }
    Ok(())
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable(format!("{label} not found"), "apk add cups"))
}

async fn run(candidates: &[&'static str], args: &[&str]) -> Result<String, ApiError> {
    let program = tool(candidates, "CUPS")?;
    exec::run_root_program(program, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
}

async fn run_quoted(candidates: &[&'static str], args: &[&str]) -> Result<String, ApiError> {
    let program = tool(candidates, "CUPS")?;
    exec::run_root_program_quoted(program, args, TIMEOUT)
        .await
        .map(|output| output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_printers_and_default() {
        let output = "printer Office is idle. enabled since now\n\tDescription: Office\n\tLocation: Desk\nsystem default destination: Office\nprinter Lab disabled since now\n";
        let printers = parse_printers(output);
        assert_eq!(printers.len(), 2);
        assert!(printers[0].is_default);
        assert_eq!(printers[0].description.as_deref(), Some("Office"));
        assert!(!printers[1].accepting);
    }

    #[test]
    fn validates_printer_fields() {
        assert!(validate_name("Office-1").is_ok());
        assert!(validate_name("-bad").is_err());
        assert!(validate_uri("ipps://printer.local/ipp/print").is_ok());
        assert!(validate_uri("file:///etc/passwd").is_err());
    }
}
