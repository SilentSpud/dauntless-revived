//! `dauntless-revived-launcher --service metagame|deploy --env-file <file>` (D12): the launcher's own binary
//! running one of offline play's services, as a child process of the launcher, so a crash in a service
//! cannot take the launcher or its relay down. Started only by `stack::ProcessRuntime`.

use std::path::PathBuf;
use std::process::ExitCode;

use super::stack::ServiceName;

/// The service these arguments ask for, if they are a `--service` call: (name, settings file).
pub fn requested(args: &[String]) -> Option<Result<(ServiceName, PathBuf), String>> {
    let (flag, rest) = args.get(1..)?.split_first()?;
    if flag != "--service" {
        return None;
    }
    Some(match rest {
        [name, env_flag, file] if env_flag == "--env-file" => ServiceName::parse(name)
            .map(|name| (name, PathBuf::from(file)))
            .ok_or_else(|| format!("unknown service {name}")),
        _ => Err("usage: --service metagame|deploy --env-file <file>".into()),
    })
}

/// Runs the requested service until it is asked to stop, if the arguments ask for one.
pub fn run_if_requested() -> Option<ExitCode> {
    let args: Vec<String> = std::env::args().collect();
    Some(match requested(&args)? {
        Ok((ServiceName::Metagame, file)) => metagame::service::serve(Some(&file)),
        Ok((ServiceName::Deploy, file)) => deploy::service::serve(Some(&file)),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn only_the_exact_service_call_runs_a_service() {
        assert_eq!(requested(&args(&["launcher"])), None);
        assert_eq!(requested(&args(&["launcher", "dauntless-revived://join?v=1"])), None);
        assert_eq!(
            requested(&args(&["launcher", "--service", "deploy", "--env-file", "d.env"])),
            Some(Ok((ServiceName::Deploy, PathBuf::from("d.env"))))
        );
        assert!(matches!(
            requested(&args(&["launcher", "--service", "shell", "--env-file", "x"])),
            Some(Err(_))
        ));
        assert!(matches!(
            requested(&args(&["launcher", "--service", "metagame"])),
            Some(Err(_))
        ));
    }
}
